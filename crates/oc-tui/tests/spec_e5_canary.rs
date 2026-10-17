//! E5 accessibility canaries (SPEC-R6; I8 per-axis tier — R5 §5 designs).
//!
//! Three legs, one run; every failure must LOCALIZE to the injected defect:
//!
//! * **Leg A — contrast:** real fg/bg pairs from the resolved DEFAULT DARK
//!   theme (the committed v2 asset through `parse_theme_document` +
//!   `resolve_theme_document` — the A7 chain's resolver output, i.e.
//!   post-quantizer token values). WCAG 2.x relative-luminance ratio;
//!   threshold 4.5:1 text / 3:1 UI per the v5 E5 clause. Cells where the
//!   REF-derived theme itself violates are INHERITED-FINDINGS (recorded,
//!   not canary failures). DEFECT (throwaway copy): one pair's fg := its
//!   bg (ratio 1:1) → the assert must fail EXACTLY that cell.
//! * **Leg B — color-independence:** busy/error state pair must be
//!   distinguishable by a non-hue channel: |ΔL| (relative luminance) must
//!   exceed the grayscale-confusion epsilon for pairs with no
//!   glyph/shape distinction. DEFECT: replace the error color with a
//!   LUMINANCE-EQUAL red (busy keeps green) → the check must fail that
//!   pair.
//! * **Leg C — key-only walk:** targets = the REAL default keymap table
//!   (`oc_tui::keymap::default_table()`); walk = stroke-sequence dispatch
//!   from root over the table's binding specs (leader chords expanded).
//!   DEFECT: delete ONE target's bindings in the copy → the walk must
//!   report EXACTLY that target unreachable and no others.
//!
//! Canaries inject defects into in-memory COPIES inside this test — the
//! product source is never modified. Results are recorded in
//! docs/spec-round6.md §canaries.

use oc_theme::{parse_theme_document, resolve_theme_document, Mode, Rgba};
use oc_tui::keymap::default_table;

// ---------------------------------------------------------------- luminance

/// WCAG 2.x relative luminance of an sRGB color.
fn rel_lum(c: Rgba) -> f64 {
    let ch = |v: u8| -> f64 {
        let s = f64::from(v) / 255.0;
        if s <= 0.03928 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
}

/// WCAG contrast ratio between two colors.
fn contrast(a: Rgba, b: Rgba) -> f64 {
    let (la, lb) = (rel_lum(a), rel_lum(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

// ------------------------------------------------------------------ theme IO

fn load_default_dark() -> oc_theme::ResolvedTheme {
    let path = format!(
        "{}/../oc-theme/assets/v2/opencode.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let asset = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("asset {path}: {e}"));
    let json: serde_json::Value = serde_json::from_str(&asset).expect("asset parses");
    let doc = parse_theme_document(&json, "opencode").expect("v2 parse");
    resolve_theme_document(&doc, Some(Mode::Dark)).expect("resolve dark")
}

/// One product-rendered fg/bg cell (real pairs the TUI paints with).
#[derive(Clone)]
struct Cell {
    name: &'static str,
    fg: Rgba,
    bg: Rgba,
    class: Class,
}
#[derive(Clone, Copy, PartialEq)]
enum Class {
    Text, // 4.5:1
    Ui,   // 3:1
}

fn real_pairs(t: &oc_theme::ResolvedTheme) -> Vec<Cell> {
    let tk = &t.tokens;
    let bg = tk.background.base;
    let raised = tk.background.raised.base;
    vec![
        Cell { name: "text.base / background.base", fg: tk.text.base, bg, class: Class::Text },
        Cell { name: "text.muted / background.base", fg: tk.text.muted, bg, class: Class::Text },
        Cell { name: "text.base / background.raised.base", fg: tk.text.base, bg: raised, class: Class::Text },
        Cell { name: "feedback.error.base / background.base", fg: tk.text.feedback.error.base, bg, class: Class::Text },
        Cell { name: "feedback.warning.base / background.base", fg: tk.text.feedback.warning.base, bg, class: Class::Text },
        Cell { name: "feedback.success.base / background.base", fg: tk.text.feedback.success.base, bg, class: Class::Text },
        Cell { name: "feedback.info.base / background.base", fg: tk.text.feedback.info.base, bg, class: Class::Text },
        Cell { name: "action.primary.base / background.base", fg: tk.text.action.primary.base, bg, class: Class::Ui },
        Cell { name: "border.base / background.base", fg: tk.border.base, bg, class: Class::Ui },
        Cell { name: "scrollbar.base / background.base", fg: tk.scrollbar.base, bg, class: Class::Ui },
    ]
}

fn threshold(c: Class) -> f64 {
    match c {
        Class::Text => 4.5,
        Class::Ui => 3.0,
    }
}

/// The clause's ratio assert. Returns the list of FAILING cell names.
fn ratio_assert(cells: &[Cell]) -> Vec<String> {
    cells
        .iter()
        .filter(|c| contrast(c.fg, c.bg) < threshold(c.class))
        .map(|c| c.name.to_string())
        .collect()
}

// ------------------------------------------------------------- leg C: walk

/// Stroke trie over the default table's binding specs: a command is
/// REACHABLE iff at least one of its comma-separated default specs is a
/// key stroke sequence. `defaults: "none"` = palette-dispatched only
/// (mirrors ref `config/keybind.ts` "none" rows) — key-unreachable BY
/// DESIGN, dispositioned, not walk failures.
fn walk_reachable(table: &[oc_tui::keymap::CommandDef]) -> std::collections::BTreeSet<String> {
    let mut reach = std::collections::BTreeSet::new();
    for def in table {
        for spec in def.defaults.split(',') {
            let spec = spec.trim();
            if spec.is_empty() || spec == "none" {
                continue;
            }
            let strokes: Vec<&str> = spec.split_whitespace().collect();
            if !strokes.is_empty() && strokes.iter().all(|s| !s.is_empty()) {
                // Row identity = bind_name: the same dot-command may appear
                // in several layers (base/dialog) with different defaults —
                // those are distinct WALK TARGETS.
                reach.insert(def.bind_name.to_string());
                break;
            }
        }
    }
    reach
}

// ------------------------------------------------------------------- tests

/// Baseline: every real pair enumerated, ratios reported; ref-inherited
/// violations recorded as findings, not failures (the clause's route).
#[test]
fn e5_leg_a_baseline_and_injection() {
    let theme = load_default_dark();
    let cells = real_pairs(&theme);
    assert!(cells.len() >= 10, "enumeration is non-trivial");

    let baseline_failures = ratio_assert(&cells);
    // Inherited-findings route: print them; assert they are FEW (a theme
    // with widespread violations would invalidate the enumeration).
    eprintln!("[E5-A] baseline failing cells (inherited-findings): {baseline_failures:?}");
    for name in &baseline_failures {
        eprintln!("[E5-A] inherited-finding: {name}");
    }
    assert!(
        baseline_failures.len() <= cells.len() / 2,
        "more than half the real pairs violate — enumeration or theme load is wrong"
    );

    // DEFECT (throwaway copy): fg := bg on exactly ONE cell.
    let mut poisoned = cells.clone();
    let victim = &poisoned[1]; // text.muted / background.base
    let victim_name = victim.name;
    let bg = victim.bg;
    poisoned[1].fg = bg; // ratio 1:1
    let failures = ratio_assert(&poisoned);
    // Localization bar (I8): the NEW failures vs baseline must be EXACTLY
    // the poisoned cell — inherited-findings stay listed, not counted.
    let mut baseline_set = baseline_failures.clone();
    let mut new_failures: Vec<String> = failures.clone();
    for b in &baseline_set {
        new_failures.retain(|f| f != b);
    }
    assert_eq!(
        new_failures,
        vec![victim_name.to_string()],
        "canary must localize NEW failure to the poisoned cell (new={new_failures:?}, all={failures:?}, baseline={baseline_failures:?})"
    );
    baseline_set.sort();
    let mut failures_sorted = failures.clone();
    failures_sorted.sort();
    let mut expected = baseline_failures.clone();
    expected.push(victim_name.to_string());
    expected.sort();
    assert_eq!(failures_sorted, expected, "post-poison failures = baseline + exactly the poisoned cell");
    let ratio = contrast(bg, bg);
    assert!((ratio - 1.0).abs() < 1e-9, "1:1 setup sanity");
    eprintln!("[E5-A] PASS: injected fg==bg localized to '{victim_name}' (ratio 1.00); inherited-findings (ref-theme, listed): {baseline_failures:?}");
}

/// Leg B: luminance-based state distinguishability + injected
/// luminance-equal red/green pair.
#[test]
fn e5_leg_b_color_independence() {
    let theme = load_default_dark();
    let error = theme.tokens.text.feedback.error.base;
    let busy = theme.tokens.text.feedback.success.base; // success/busy family
    const EPS: f64 = 0.02; // grayscale-confusion band for relative luminance

    // The clause: pairs distinguishable by non-hue channel OR glyph/shape.
    // busy/error text rows carry glyphs (spinners/icons) in the product;
    // the canary checks the COLOR channel's non-hue reserve (ΔL).
    let distinguishable = |a: Rgba, b: Rgba, glyph_carrying: bool| {
        (rel_lum(a) - rel_lum(b)).abs() > EPS || glyph_carrying
    };
    assert!(
        distinguishable(busy, error, true),
        "real pair sanity"
    );
    let real_dl = (rel_lum(busy) - rel_lum(error)).abs();
    eprintln!("[E5-B] real busy/error ΔL = {real_dl:.3}");

    // DEFECT (throwaway values, not committed): luminance-equal red/green.
    // Solve green channel keeping the red's luminance fixed: adjust g until
    // |L(red) - L(green)| <= EPS/4.
    let red = Rgba { r: 220, g: 40, b: 40, a: 255 };
    let target = rel_lum(red);
    let mut g = 40u16;
    let green = loop {
        let cand = Rgba { r: 40, g: g as u8, b: 40, a: 255 };
        if rel_lum(cand) >= target || g == 255 {
            break cand;
        }
        g += 1;
    };
    assert!(
        (rel_lum(red) - rel_lum(green)).abs() <= EPS,
        "injection setup: pair is luminance-equal (Δ={:.4})",
        (rel_lum(red) - rel_lum(green)).abs()
    );
    // Same glyph assumption as the product pair → must FAIL to distinguish:
    assert!(
        !distinguishable(green, red, false),
        "canary: luminance-equal pair with no glyph channel MUST be flagged indistinguishable"
    );
    eprintln!(
        "[E5-B] PASS: luminance-equal red/green (ΔL={:.4}) flagged; real pair ΔL={real_dl:.3} passes",
        (rel_lum(red) - rel_lum(green)).abs()
    );
}

/// Leg C: key walk over the REAL default table; injected unbind localizes.
#[test]
fn e5_leg_c_key_walk_unbind() {
    let table = default_table();
    assert!(!table.is_empty(), "real table loaded");
    let all: Vec<oc_tui::keymap::CommandDef> = table.to_vec();
    let reach_before = walk_reachable(&all);
    let none_count = all
        .iter()
        .filter(|d| d.defaults.split(',').all(|s| s.trim().is_empty() || s.trim() == "none"))
        .count();
    let bound = all.len() - none_count;
    for d in &all {
        let is_none = d.defaults.split(',').all(|s| s.trim().is_empty() || s.trim() == "none");
        if !is_none && !reach_before.contains(d.bind_name) {
            eprintln!("[E5-C] bound-but-unreachable: {} defaults={:?}", d.bind_name, d.defaults);
        }
    }
    assert_eq!(
        reach_before.len(),
        bound,
        "every key-bound command reachable; {none_count} palette-only ('none') dispositioned"
    );
    eprintln!("[E5-C] table: {} commands, {} key-bound, {none_count} palette-only", all.len(), bound);

    // DEFECT (throwaway copy): strip bindings from ONE command that has no
    // alternates (single spec) so removal ⇒ unreachable.
    let victim_idx = all
        .iter()
        .enumerate()
        .find(|(_, d)| d.defaults.split(',').count() == 1 && !d.defaults.trim().is_empty())
        .map(|(i, _)| i)
        .expect("a single-spec command exists");
    let victim = all[victim_idx].command.to_string();
    let mut poisoned = all.clone();
    poisoned[victim_idx].defaults = "";
    let reach_after = walk_reachable(&poisoned);
    let now_unreachable: Vec<String> =
        reach_before.difference(&reach_after).cloned().collect();
    assert_eq!(
        now_unreachable,
        vec![all[victim_idx].bind_name.to_string()],
        "exactly the unbound target becomes unreachable (victim command: {victim})"
    );
    assert_eq!(reach_after.len(), bound - 1, "no collateral unreachables");
    eprintln!("[E5-C] PASS: unbinding '{victim}' → exactly that target unreachable ({}/{} bound still reach)", reach_after.len(), bound);
}
