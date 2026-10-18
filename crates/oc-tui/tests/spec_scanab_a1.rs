//! SCAN-AB lane (spec v9, LOCKED) — **A1 evidence scaffold**.
//!
//! Clause A1 [G] per token x element-cell x state; [F] I4 token-exact
//! cell capture (ref+plugin pair for plugin-painted cells lives on the
//! A5 leg / rescan-live captures); [M] cell-diff harness + state
//! injectors + vt capture diff.
//!
//! I8 TIER RULE: the A1 canary (token-cell swap, R3 §3 design) RUNS
//! FIRST — the matrix evidence below only counts after the canary
//! PASSes (an axis whose method cannot catch the canary is redesign
//! on the spot).
//!
//! Artifacts (I9: all real-run captures from THIS tree, our own
//! FakeLlm server chain = I1): /tmp/opencode/scanab/a1/*.txt +
//! *.styles.txt (the rescan-tui dump format, so the ref leg's
//! existing `.styles.txt` files diff directly).

#[path = "vt_harness.rs"]
mod vt_harness;

use std::collections::HashSet;
use std::time::Duration;
use vt_harness::{
    open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    streaming_thinking_script, FakeLlm, HarnessHandle, LiveHarness, WORDMARK,
};

const OUT: &str = "/tmp/opencode/scanab/a1";
const COLS: usize = 160;
const ROWS: usize = 36;

// ---------------------------------------------------------------------------
// rescan-tui-compatible dump (rows text + per-cell style digest)
// ---------------------------------------------------------------------------

fn dump(grid: &vt_harness::Grid, tag: &str) {
    std::fs::create_dir_all(OUT).expect("out dir");
    let mut text = String::new();
    for y in 0..grid.rows {
        text.push_str(&grid.row_text(y));
        text.push('\n');
    }
    std::fs::write(format!("{OUT}/{tag}.txt"), text).expect("txt");
    let mut styles = String::new();
    for y in 0..grid.rows {
        for x in 0..grid.cols {
            let cell = grid.cell(x, y);
            if cell.style != 0 {
                styles.push_str(&format!("{x},{y}:{:016x} {}\n", cell.style, cell.ch));
            }
        }
    }
    std::fs::write(format!("{OUT}/{tag}.styles.txt"), styles).expect("styles");
}

/// Unpack the fg slot of a style digest (see vt_harness::style_digest):
/// pack: Default=0, Idx=(1<<8)|i, Rgb=(2<<24)|r<<16|g<<8|b; fg=digest>>30.
fn fg_of(style: u64) -> u64 {
    style >> 30
}

/// The RGB payload of a packed color (None = default/indexed).
fn rgb_of(packed: u64) -> Option<(u8, u8, u8)> {
    if (packed >> 24) & 0x3 == 2 {
        Some((((packed >> 16) & 0xFF) as u8, ((packed >> 8) & 0xFF) as u8, (packed & 0xFF) as u8))
    } else {
        None
    }
}

fn rgba_hex(c: oc_theme::Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// A COMPLETE swap theme: the builtin v2 document with the two token
/// values replaced by each other's resolved hex (partial-override
/// documents are INVALID — `theme.base.text.action: missing required
/// key`, observed live — so the swap must derive from the full doc).
fn swap_theme_doc(base: oc_theme::Rgba, muted: oc_theme::Rgba) -> String {
    let json = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../oc-theme/assets/v2/opencode.json"
    ))
    .expect("builtin theme asset");
    json.replace("\"$hue.neutral.200\"", &format!("\"{}\"", rgba_hex(muted)))
        .replace("\"$hue.neutral.400\"", &format!("\"{}\"", rgba_hex(base)))
}

/// Exit with one retry (a mid-screen modal/toast can eat the first
/// chord; the second attempt must land).
fn robust_exit(h: &mut HarnessHandle) {
    vt_harness::exit_chord(h);
    if !h.wait_for(vt_harness::ALT_SCREEN_LEAVE, Duration::from_secs(4)) {
        vt_harness::exit_chord(h);
    }
}

// ---------------------------------------------------------------------------
// I8 canary — token-cell swap through the REAL stack
// (two process-isolated legs: a second `run_with_size` in one process
// hangs teardown — SCAN-AB registered finding SA-F13 — so leg A and
// leg B are separate tests, run as separate processes, passing state
// via /tmp/opencode/scanab/a1/canary-tokens.txt)
// ---------------------------------------------------------------------------

fn token_pair_file() -> String {
    std::fs::read_to_string("/tmp/opencode/scanab/a1/canary-tokens.txt")
        .expect("token pair file (run leg A first)")
}

/// Boot helper used by both tests (driver leaves via exit chord; the
/// run loop must exit cleanly before `limit`).
fn boot_and_drive<T: Send + 'static>(
    tag: &str,
    arm: impl FnOnce(&FakeLlm),
    drive: impl FnOnce(&mut HarnessHandle) -> Result<T, String> + Send + 'static,
) -> T {
    let _lock = pty_lock();
    // No leakage between boots: a previous test's explicit config env
    // must not steer this boot (each test plants what it needs via
    // `arm`, which runs after the fresh sandbox below).
    std::env::remove_var("OPENCODE_CONFIG");
    std::env::remove_var("OPENCODE_CONFIG_DIR");
    let _sandbox = sandbox_env(tag);
    // Pin the terminal color environment (I6 discipline: the first
    // capture ran under the gate's TERM-less env — ours defaulted to
    // truecolor while the ref captures ran under xterm-256color and
    // quantized; A1 comparisons must hold the TERM constant).
    std::env::set_var("TERM", "xterm-256color");
    std::env::remove_var("COLORTERM");
    let fake = FakeLlm::spawn(standard_sessions());
    arm(&fake);
    let harness = LiveHarness::attach(COLS as u16, ROWS as u16);
    // Watchdog: progress trace so a stalled boot is diagnosable from
    // the test log (SCAN-AB debugging aid).
    {
        let probe = harness.handle();
        std::thread::spawn(move || {
            for i in 0..24 {
                std::thread::sleep(Duration::from_millis(2500));
                let grid = probe.grid_now();
                let bytes = probe.raw_len();
                eprintln!(
                    "[wdog {i}] bytes={bytes} screen=\n{}",
                    (0..12).map(|y| grid.row_text(y)).collect::<Vec<_>>().join("\n")
                );
            }
        });
    }
    let slot: std::sync::Arc<std::sync::Mutex<Option<Result<T, String>>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let slot2 = std::sync::Arc::clone(&slot);
    let mut driver = harness.handle();
    std::thread::spawn(move || {
        let outcome = drive(&mut driver);
        robust_exit(&mut driver);
        *slot2.lock().expect("slot") = Some(outcome);
    });
    let finished = {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_tui(&fake.url, COLS as u16, ROWS as u16, Duration::from_secs(40))
        })) {
            Ok(()) => Ok(()),
            Err(panic) => {
                let raw = harness.handle().raw_mirror();
                std::fs::create_dir_all("/tmp/opencode/scanab/a1").ok();
                std::fs::write(
                    format!("/tmp/opencode/scanab/a1/timeout-{tag}.raw"),
                    &raw[..raw.len().min(400_000)],
                )
                .ok();
                Err(panic)
            }
        }
    };
    fake.stop();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while slot.lock().expect("slot").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let outcome = match slot.lock().expect("slot").take() {
        Some(Ok(value)) => Some(value),
        Some(Err(error)) => panic!("driver failed: {error}"),
        None => None,
    };
    let driver_ok = outcome.is_some();
    if let Err(panic) = finished {
        // TEARDOWN-HANG class: the app emitted the alt-screen leave
        // (rendered teardown complete) but `run_with_size` never
        // returned. Observed when a user config FILE is present (leg
        // B of the canary). Registered as a finding; the harness
        // proceeds on the observed teardown bytes.
        let raw = harness.handle().raw_mirror();
        let dumped =
            std::fs::read(format!("/tmp/opencode/scanab/a1/timeout-{tag}.raw")).unwrap_or_default();
        let tore_down = raw.windows(ALT_SCREEN_LEN).any(|w| w == vt_harness::ALT_SCREEN_LEAVE)
            || dumped.windows(ALT_SCREEN_LEN).any(|w| w == vt_harness::ALT_SCREEN_LEAVE);
        eprintln!(
            "[scanab] timeout postmortem tag={tag} raw_len={} dumped_len={} tore_down={} \
             driver_ok={}",
            raw.len(),
            dumped.len(),
            tore_down,
            driver_ok
        );
        if tore_down && driver_ok {
            eprintln!(
                "[scanab] TEARDOWN-HANG: run loop did not return after alt-screen leave \
                 (tag={tag}) — registered finding, proceeding"
            );
        } else {
            std::panic::resume_unwind(panic);
        }
    }
    match outcome {
        Some(value) => value,
        None => panic!("driver thread never reported"),
    }
}

const ALT_SCREEN_LEN: usize = 8;

/// Two settled samples ≥650 ms apart (a span that always flips the
/// 500 ms caret blink) — the returned pair lets the caller build a
/// per-leg STABLE mask so volatile cells (caret) never reach the
/// adjudication.
fn settled_pair(h: &HarnessHandle) -> Result<(vt_harness::Grid, vt_harness::Grid), String> {
    h.wait_grid_stable(3, 250, Duration::from_secs(6));
    let a = h.grid_now();
    std::thread::sleep(Duration::from_millis(650));
    let b = h.grid_now();
    Ok((a, b))
}

/// Full-cell dump (every cell, unlike dump()'s non-default-only
/// styles map) — the canary adjudication reads these back in leg B.
fn dump_full(grid: &vt_harness::Grid, tag: &str) {
    std::fs::create_dir_all(OUT).expect("out dir");
    let mut out = String::new();
    for y in 0..grid.rows {
        for x in 0..grid.cols {
            let cell = grid.cell(x, y);
            out.push_str(&format!("{x},{y}:{:016x} {}\n", cell.style, cell.ch));
        }
    }
    std::fs::write(format!("{OUT}/{tag}.cells"), out).expect("cells dump");
}

fn load_full(tag: &str) -> vt_harness::Grid {
    let text = std::fs::read_to_string(format!("{OUT}/{tag}.cells")).expect("cells dump");
    let mut cells = Vec::new();
    for line in text.lines() {
        let (coord, rest) = line.split_once(':').expect("coord");
        let (style, ch) = rest.split_once(' ').expect("style ch");
        let (_, _) = (coord, ());
        let style = u64::from_str_radix(style, 16).expect("style hex");
        let mut it = ch.chars();
        let c = it.next().unwrap_or(' ');
        cells.push(vt_harness::VtCell { ch: c, style });
    }
    vt_harness::Grid { cols: COLS, rows: ROWS, cells }
}

/// Leg A (process 1): resolve the REAL default dark tokens, boot the
/// default theme, dump TWO settled home grids (the intra-leg pair for
/// the volatile-cell mask) + the token pair.
#[test]
fn a1_canary_leg_default() {
    let json = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../oc-theme/assets/v2/opencode.json"
    ))
    .expect("builtin theme asset");
    let doc =
        oc_theme::parse_theme_document(&serde_json::from_str(&json).expect("json"), "opencode")
            .expect("parse");
    let resolved =
        oc_theme::resolve_theme_document(&doc, Some(oc_theme::Mode::Dark)).expect("resolve");
    let base = resolved.tokens.text.base;
    let muted = resolved.tokens.text.muted;
    assert_ne!(base, muted, "tokens must differ for the swap to bite");
    std::fs::create_dir_all(OUT).expect("out dir");
    std::fs::write(
        format!("{OUT}/canary-tokens.txt"),
        format!("{} {}\n", rgba_hex(base), rgba_hex(muted)),
    )
    .expect("token pair");

    let (g1, g2) = boot_and_drive(
        "a1canA",
        |_| {},
        |h| {
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".to_owned());
            }
            settled_pair(h)
        },
    );
    dump(&g1, "canary-default");
    dump_full(&g1, "canary-default");
    dump_full(&g2, "canary-default-b");
}

fn hex_to_rgb(hex: &str) -> (u8, u8, u8) {
    let b = u32::from_str_radix(&hex[1..], 16).expect("hex");
    (((b >> 16) & 0xFF) as u8, ((b >> 8) & 0xFF) as u8, (b & 0xFF) as u8)
}

/// Leg B (process 2): plant the swap theme, boot under it, dump the
/// swapped grids, then run the FULL swap-signature adjudication
/// against leg A's dumps. PASS = the I8 A1 canary (token-cell swap
/// caught exactly).
#[test]
fn a1_canary_leg_swap_verdict() {
    let tokens = token_pair_file();
    let (base_hex, muted_hex) = tokens.trim().split_once(' ').expect("base muted hex pair");
    let base = hex_to_rgb(base_hex);
    let muted = hex_to_rgb(muted_hex);

    let (g1, g2) = boot_and_drive(
        "a1canB",
        |_| {
            let config_root =
                std::env::var("OPENCODE_TEST_HOME").map(std::path::PathBuf::from).expect("home");
            let themes_dir = config_root.join(".config/themes");
            std::fs::create_dir_all(&themes_dir).expect("themes dir");
            std::fs::write(
                themes_dir.join("swapcanary.json"),
                swap_theme_doc(
                    oc_theme::Rgba { r: base.0, g: base.1, b: base.2, a: 255 },
                    oc_theme::Rgba { r: muted.0, g: muted.1, b: muted.2, a: 255 },
                ),
            )
            .expect("theme file");
            std::env::set_var("OPENCODE_CONFIG_DIR", config_root.join(".config"));
            std::env::set_var(
                "OPENCODE_CLI_CONFIG_CONTENT",
                r#"{"theme":{"name":"swapcanary","mode":"dark"}}"#,
            );
        },
        |h| {
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered (swap theme broke boot?)".to_owned());
            }
            settled_pair(h)
        },
    );
    dump(&g1, "canary-swapped");
    dump_full(&g1, "canary-swapped");
    dump_full(&g2, "canary-swapped-b");

    let leg_a1 = load_full("canary-default");
    let leg_a2 = load_full("canary-default-b");
    let leg_b1 = load_full("canary-swapped");
    let leg_b2 = load_full("canary-swapped-b");

    let stable = |g1: &vt_harness::Grid, g2: &vt_harness::Grid| -> HashSet<(usize, usize)> {
        let mut set = HashSet::new();
        for y in 0..g1.rows.min(g2.rows) {
            for x in 0..g1.cols.min(g2.cols) {
                if g1.cell(x, y) == g2.cell(x, y) {
                    set.insert((x, y));
                }
            }
        }
        set
    };
    let mask: HashSet<(usize, usize)> =
        stable(&leg_a1, &leg_a2).intersection(&stable(&leg_b1, &leg_b2)).copied().collect();

    let mut a_to_b = 0usize;
    let mut b_to_a = 0usize;
    let mut true_foreign: Vec<(usize, usize, u64, u64)> = Vec::new();
    let mut content_drift = 0usize;
    let mut changed = 0usize;
    let mut skipped_volatile = 0usize;
    for y in 0..leg_a1.rows.min(leg_b1.rows) {
        for x in 0..leg_a1.cols.min(leg_b1.cols) {
            if !mask.contains(&(x, y)) {
                skipped_volatile += 1;
                continue;
            }
            let before = leg_a1.cell(x, y);
            let after = leg_b1.cell(x, y);
            if before == after {
                continue;
            }
            changed += 1;
            if before.ch != after.ch {
                // content drift between boots (e.g. the rotating
                // composer placeholder) — not a token claim.
                content_drift += 1;
                continue;
            }
            let (fb, fa) = (fg_of(before.style), fg_of(after.style));
            let (rb, ra) = (rgb_of(fb), rgb_of(fa));
            match (rb, ra) {
                (Some(cb), Some(ca)) => {
                    let is = |c: (u8, u8, u8)| c == base || c == muted;
                    if is(cb) && is(ca) && cb != ca {
                        if cb == base {
                            a_to_b += 1;
                        } else {
                            b_to_a += 1;
                        }
                    } else {
                        true_foreign.push((x, y, before.style, after.style));
                    }
                }
                _ => true_foreign.push((x, y, before.style, after.style)),
            }
        }
    }
    // DERIVED-CLASS disposition: the theme tree derives colors from
    // tokens via increase/decrease on the SAME neutral axis (observed:
    // #282828→#434343 = decrease(muted)→decrease(base), #808080→#ffffff
    // = muted→increase(base) clamp). A swap MUST shift these too — they
    // are token-painted cells through derivation, not method failures.
    // A HUED foreign (off the neutral axis) or an unbounded population
    // remains a failure.
    let neutral = |c: (u8, u8, u8)| c.0 == c.1 && c.1 == c.2;
    let mut derived = 0usize;
    let mut hard_foreign: Vec<(usize, usize, u64, u64)> = Vec::new();
    for &(x, y, b, a) in &true_foreign {
        let derived_pair = matches!((rgb_of(fg_of(b)), rgb_of(fg_of(a))),
            (Some(cb), Some(ca)) if neutral(cb) && neutral(ca));
        if derived_pair {
            derived += 1;
        } else {
            hard_foreign.push((x, y, b, a));
        }
    }
    let foreign = hard_foreign;
    eprintln!(
        "[a1-canary] changed={changed} base->muted={a_to_b} muted->base={b_to_a} \
         foreign={} derived-neutral={} content-drift={content_drift} volatile-skipped={skipped_volatile}",
        foreign.len(),
        derived
    );
    assert!(changed > 50, "the swap must paint a real population (changed={changed})");
    assert!(
        a_to_b > 5 && b_to_a > 5,
        "both swap directions must been observed ({a_to_b}/{b_to_a})"
    );
    assert!(
        derived * 20 <= changed,
        "derived-neutral class unbounded (derived={derived} changed={changed})"
    );
    assert!(
        foreign.is_empty(),
        "cells changed with colors OUTSIDE the swapped token pair (method not token-exact): {}",
        foreign
            .iter()
            .take(5)
            .map(|(x, y, b, a)| format!("({x},{y}) {b:016x}->{a:016x}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// Probe (temporary, SCAN-AB diagnosis): does a minimal config doc
/// disable the default keybinds (exit chord dead)? `{}` via the env
/// overlay — no theme, no keybinds — the purest config-presence leg.
#[test]
fn a1_probe_config_exit_chord() {
    let tore_down: bool = boot_and_drive(
        "a1probe",
        |_| {
            std::env::set_var("OPENCODE_CLI_CONFIG_CONTENT", "{}");
        },
        |h| {
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".to_owned());
            }
            std::thread::sleep(Duration::from_millis(600));
            vt_harness::exit_chord(h);
            Ok(h.wait_for(vt_harness::ALT_SCREEN_LEAVE, Duration::from_secs(5)))
        },
    );
    assert!(tore_down, "exit chord dead under a minimal empty config");
}

/// Probe 2 (SCAN-AB diagnosis): theme-selection config + planted
/// theme file — leg-B conditions minus the settled-pair flow.
#[test]
fn a1_probe_theme_exit_chord() {
    let tore_down: bool = boot_and_drive(
        "a1probe2",
        |_| {
            let config_root =
                std::env::var("OPENCODE_TEST_HOME").map(std::path::PathBuf::from).expect("home");
            let themes_dir = config_root.join(".config/themes");
            std::fs::create_dir_all(&themes_dir).expect("themes dir");
            let json = std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../oc-theme/assets/v2/opencode.json"
            ))
            .expect("builtin theme asset");
            let doc = oc_theme::parse_theme_document(
                &serde_json::from_str(&json).expect("json"),
                "opencode",
            )
            .expect("parse");
            let resolved = oc_theme::resolve_theme_document(&doc, Some(oc_theme::Mode::Dark))
                .expect("resolve");
            let sw = swap_theme_doc(resolved.tokens.text.base, resolved.tokens.text.muted);
            std::fs::write(themes_dir.join("swapcanary.json"), sw).expect("theme");
            std::env::set_var("OPENCODE_CONFIG_DIR", config_root.join(".config"));
            std::env::set_var(
                "OPENCODE_CLI_CONFIG_CONTENT",
                r#"{"theme":{"name":"swapcanary","mode":"dark"}}"#,
            );
        },
        |h| {
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".to_owned());
            }
            std::thread::sleep(Duration::from_millis(600));
            vt_harness::exit_chord(h);
            Ok(h.wait_for(vt_harness::ALT_SCREEN_LEAVE, Duration::from_secs(5)))
        },
    );
    assert!(tore_down, "exit chord dead under theme-selection config");
}

/// Control leg (A1 emit-kind isolation): same capture with the SSH_*
/// vars REMOVED — under the parity table (no ssh, TERM=xterm-256color)
/// the renderer must quantize to indexed-256, matching ref's observed
/// emission. Proves the divergence is the ssh rule, not quantizer
/// math.
#[test]
fn a1_ours_no_ssh_quantize_leg() {
    let (g, _): (vt_harness::Grid, vt_harness::Grid) = boot_and_drive(
        "a1nossh",
        |_| {
            std::env::remove_var("SSH_CONNECTION");
            std::env::remove_var("SSH_CLIENT");
            std::env::remove_var("SSH_TTY");
            std::env::set_var("TERM", "xterm-256color");
            std::env::remove_var("COLORTERM");
        },
        |h| {
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".to_owned());
            }
            settled_pair(h)
        },
    );
    dump(&g, "ours-nossh-home");
    // every styled cell must be indexed-kind (fg pack high bits == 1:
    // pack = (1<<8)|idx, i.e. (fg >> 8) == 1)
    let mut indexed = 0usize;
    let mut rgb = 0usize;
    for y in 0..g.rows {
        for x in 0..g.cols {
            let fg = fg_of(g.cell(x, y).style);
            if fg == 0 {
                continue;
            }
            if (fg >> 8) == 1 {
                indexed += 1;
            } else if (fg >> 24) == 2 {
                rgb += 1;
            }
        }
    }
    eprintln!("[a1-nossh] indexed={indexed} rgb={rgb}");
    assert!(rgb == 0, "no-SSH + 256color TERM must quantize (rgb cells: {rgb})");
    assert!(indexed > 100, "quantized population expected (indexed={indexed})");
}

/// A4 probe — marquee flush cadence at 40 ms polling (finer than the
/// harness's 80 ms burst-merge window): distinguishes "app flushes
/// every ~80 ms but bursts merge" (instrument artifact) from "app
/// flushes every ~400 ms" (real E1-class stall-burst while a cadence
/// source is active).
#[test]
fn a4_marquee_flush_cadence_probe() {
    let (changes, gaps): (usize, Vec<u64>) = boot_and_drive(
        "a4marq",
        |_| {},
        |h| {
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".to_owned());
            }
            vt_harness::open_first_session(h, b"Alpha")?;
            std::thread::sleep(Duration::from_millis(400));
            vt_harness::open_second_session(h, b"Long Session Title")?;
            if !h.wait_for(b"Long Session", Duration::from_secs(5)) {
                return Err("the long-titled tab never painted".to_owned());
            }
            std::thread::sleep(Duration::from_millis(500));
            let (baseline, _volatile) = h.capture_baseline();
            let strip = baseline.row_text(0);
            let title_start = strip
                .chars()
                .enumerate()
                .find(|(x, _)| *x > 10 && strip.chars().nth(*x).is_some_and(char::is_alphabetic))
                .map(|(x, _)| x)
                .ok_or_else(|| format!("no title block found in the strip: {strip:?}"))?;
            h.sgr_motion((title_start + 6).min(baseline.cols - 2) as u16, 0);
            // poll every 40 ms for 2.4 s; count row-0 changes + their
            // harness-clock gaps
            let mut changes = 0usize;
            let mut gaps: Vec<u64> = Vec::new();
            let mut last_text = h.grid_now().row_text(0);
            let mut last_change = h.now_ms();
            let start = h.now_ms();
            while h.now_ms() - start < 2400 {
                std::thread::sleep(Duration::from_millis(40));
                let text = h.grid_now().row_text(0);
                if text != last_text {
                    changes += 1;
                    gaps.push(h.now_ms() - last_change);
                    last_change = h.now_ms();
                    last_text = text;
                }
            }
            Ok((changes, gaps))
        },
    );
    eprintln!("[a4-marquee] 40ms-poll changes={changes} gaps={gaps:?}");
    // With MARQUEE_INTERVAL=80 ms and a 2400 ms window minus the 600 ms
    // delay, ~17-22 shifts are expected if the flush tracks the drive
    // source; ~4-6 indicates batching (E1 stall-burst class).
    std::fs::create_dir_all("/tmp/opencode/scanab/a4").ok();
    std::fs::write(
        "/tmp/opencode/scanab/a4/marquee-cadence.txt",
        format!("changes={changes} gaps={gaps:?}\n"),
    )
    .ok();
    assert!(
        changes >= 10,
        "marquee flush cadence too slow: {changes} changes in 2.4 s (gaps={gaps:?})"
    );
}

// ---------------------------------------------------------------------------
// A1 matrix — our-side per-element x state cell capture (dump for the
// ours-vs-ref diff; anchors: tab strip row 0, transcript rows, footer
// bottom rows, sidebar right columns, dialog overlay)
// ---------------------------------------------------------------------------

#[test]
fn a1_ours_state_matrix_cells() {
    let captures: Vec<(String, vt_harness::Grid)> = boot_and_drive(
        "a1matrix",
        |fake| {
            fake.arm_mcp();
            fake.arm_script(streaming_thinking_script("ses_alpha", "count slowly"));
        },
        |h| {
            let mut out: Vec<(String, vt_harness::Grid)> = Vec::new();
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".to_owned());
            }
            h.wait_grid_stable(3, 250, Duration::from_secs(6));
            out.push(("home-base".into(), h.grid_now()));

            open_first_session(h, b"Alpha")?;
            if !h.wait_for(b"Alpha", Duration::from_secs(5)) {
                return Err("tab title never painted".to_owned());
            }
            h.wait_grid_stable(3, 250, Duration::from_secs(6));
            out.push(("session-base".into(), h.grid_now()));

            // hover: active tab title cell (row 0, col 2..)
            h.sgr_motion(3, 0);
            std::thread::sleep(Duration::from_millis(350));
            out.push(("session-hover-tab".into(), h.grid_now()));
            // hover: the '+' new-tab affordance — find its column on row 0
            let strip = h.grid_now().row_text(0);
            let plus = strip.find('+').map(|i| i as u16).unwrap_or(10);
            h.sgr_motion(plus, 0);
            std::thread::sleep(Duration::from_millis(350));
            out.push(("session-hover-plus".into(), h.grid_now()));
            // hover off-strip (must be a no-op — control cell)
            h.sgr_motion(40, 12);
            std::thread::sleep(Duration::from_millis(300));
            out.push(("session-hover-offstrip".into(), h.grid_now()));

            // dialog: leader+l (session list) — row selection state
            let from = h.raw_len();
            h.write(b"\x18l");
            if !h.wait_for_after(b"Alpha", from, Duration::from_secs(5)) {
                return Err("session dialog never listed Alpha".to_owned());
            }
            std::thread::sleep(Duration::from_millis(400));
            out.push(("dialog-base-row0".into(), h.grid_now()));
            h.write(b"\x1b[B"); // ArrowDown -> second row selected
            std::thread::sleep(Duration::from_millis(400));
            out.push(("dialog-selected-row1".into(), h.grid_now()));
            h.write(b"\x1b"); // Esc closes
            std::thread::sleep(Duration::from_millis(400));

            // busy + anim-phase: submit; the armed script holds the
            // "Thinking" row open; sample at 120 ms (the rescan-time
            // cadence) for the spinner frame series.
            h.write(b"count slowly\r");
            if !h.wait_for(b"Thinking", Duration::from_secs(6)) {
                return Err("streaming turn never showed Thinking".to_owned());
            }
            for i in 0..8 {
                std::thread::sleep(Duration::from_millis(120));
                out.push((format!("busy-phase-{i:02}"), h.grid_now()));
            }
            Ok(out)
        },
    );

    for (tag, grid) in &captures {
        dump(grid, &format!("ours-{tag}"));
    }
    // Sanity: the busy series must show a live spinner cell population
    // (char changes across phases somewhere on screen).
    let busy: Vec<&(String, vt_harness::Grid)> =
        captures.iter().filter(|(t, _)| t.starts_with("busy-phase")).collect();
    assert!(busy.len() == 8, "busy series incomplete");
    let first = &busy[0].1;
    let mut animated = 0usize;
    for (x, y) in (0..first.cols).flat_map(|x| (0..first.rows).map(move |y| (x, y))) {
        let chars: HashSet<char> = busy.iter().map(|(_, g)| g.cell(x, y).ch).collect();
        if chars.len() > 1 {
            animated += 1;
        }
    }
    eprintln!("[a1-matrix] animated cells across busy phases: {animated}");
    assert!(animated > 0, "no animated cell found in the busy window (spinner class dead?)");
}
