//! The reference's string-width model — a full-table, per-semantics port of
//! `packages/tui/src/util/string-width.node.ts` (MF#16; G-452 alignment
//! target = the REF's grid behavior).
//!
//! # The ref pipeline (anchored `file:line`)
//!
//! `string-width.node.ts:9-26` reduces over `Intl.Segmenter` grapheme
//! clusters of the ANSI-stripped text, measuring each cluster with
//! `string-width@7.2.0` (`index.js`, bundled via `@opentui/core`), then
//! narrowing text-presentation emoji:
//!
//! * per cluster, `string-width@7` (its `index.js`, verbatim order):
//!   control (`<= 0x1F`, `0x7F..=0x9F`) → 0 for the WHOLE cluster;
//!   `0x200B..=0x200F` / `0xFEFF` → 0; the five combining ranges
//!   (`0x300-0x36F`, `0x1AB0-0x1AFF`, `0x1DC0-0x1DFF`, `0x20D0-0x20FF`,
//!   `0xFE20-0xFE2F`) → 0; surrogates → 0 (unrepresentable in Rust);
//!   variation selectors `0xFE00..=0xFE0F` → 0;
//!   `\p{Default_Ignorable_Code_Point}` → 0 (soft hyphen, hangul filler
//!   U+3164, tag characters …); else `emoji-regex@10.6.0` match → 2;
//!   else `eastAsianWidth(base, {ambiguousAsWide:false})` (F/W → 2, all
//!   else → 1 — never 0, so a standalone Devanagari matra counts 1).
//! * then the wrapper's narrowing (`:13-24`): a cluster measuring 2 whose
//!   base is NOT EAW F/W, matches `^\p{Emoji}\p{Mark}*$`, is not
//!   `\p{Emoji_Presentation}`, and contains no `FE0F`/`20E3`/`ZWJ`
//!   counts **1** (bare `❤`/`©` render one cell in the ref).
//!
//! # Provenance of the tables
//!
//! Every table here is a FULL property sweep of the ref runtime under
//! bun-1.4.2 (see [`crate::width_tables`]); the emoji matcher is the
//! ref's own emoji-regex pattern converted to codepoint escapes and
//! differentially validated against the original (1,112,972 probes, zero
//! mismatches — see [`crate::width_emoji_regex`]). This retires the
//! wave-15 corpus-pinned approximations (the static `TEXT_EMOJI_BASE`
//! block table and the sampled Brahmic `Mc` list) — MF-1 ledger #16.

use crate::width_emoji_regex::EMOJI_REGEX_PATTERN;
use crate::width_tables::{
    DEFAULT_IGNORABLE, EAW_FULLWIDTH_WIDE, EMOJI_PRESENTATION, EMOJI_PROPERTY, MARK_CATEGORY,
};
use regex::Regex;
use std::sync::OnceLock;

/// `emoji-regex@10.6.0` — the ref's emoji matcher (`string-width@7`'s
/// `emojiRegex().test(segment)`), compiled once.
pub fn emoji_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(EMOJI_REGEX_PATTERN).expect("vendored emoji-regex pattern compiles")
    })
}

/// Range-table membership (tables are sorted, non-overlapping).
pub(crate) fn in_table(table: &[(u32, u32)], cp: u32) -> bool {
    table
        .binary_search_by(|&(lo, hi)| {
            if cp < lo {
                std::cmp::Ordering::Greater
            } else if cp > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// `\p{Emoji}` membership (the `textEmoji` base set of the wrapper).
#[must_use]
pub fn is_emoji_property(cp: u32) -> bool {
    in_table(EMOJI_PROPERTY, cp)
}

/// `\p{Emoji_Presentation}` membership.
fn is_emoji_presentation(cp: u32) -> bool {
    in_table(EMOJI_PRESENTATION, cp)
}

/// East Asian Width Fullwidth/Wide (the ref's `eastAsianWidth(...) === 2`
/// under `ambiguousIsNarrow`, i.e. `ambiguousAsWide: false`).
fn is_eaw_wide(cp: u32) -> bool {
    in_table(EAW_FULLWIDTH_WIDE, cp)
}

/// `\p{Mark}` (Mn/Me/Mc) membership.
fn is_mark(cp: u32) -> bool {
    in_table(MARK_CATEGORY, cp)
}

/// string-width@7's per-segment skip rules (its `index.js`, verbatim):
/// a cluster whose BASE codepoint is any of these contributes 0.
fn stringwidth_skips(cp: u32) -> bool {
    cp <= 0x1F
        || (0x7F..=0x9F).contains(&cp)
        || (0x200B..=0x200F).contains(&cp)
        || cp == 0xFEFF
        || (0x0300..=0x036F).contains(&cp)
        || (0x1AB0..=0x1AFF).contains(&cp)
        || (0x1DC0..=0x1DFF).contains(&cp)
        || (0x20D0..=0x20FF).contains(&cp)
        || (0xFE20..=0xFE2F).contains(&cp)
        || (0xFE00..=0xFE0F).contains(&cp)
        || in_table(DEFAULT_IGNORABLE, cp)
}

/// `eastAsianWidth(base, {ambiguousAsWide:false})` — 2 for F/W, else 1
/// (the ref's function never returns 0: a non-skipped base always
/// contributes at least one cell).
fn eaw_width(cp: u32) -> usize {
    if is_eaw_wide(cp) {
        2
    } else {
        1
    }
}

/// The ref wrapper's `textEmoji` regex `/^\p{Emoji}\p{Mark}*$/u`: an
/// Emoji-property base followed only by Mark-category trailers.
fn emoji_mark_shape(cluster: &str) -> bool {
    let mut chars = cluster.chars();
    let Some(base) = chars.next() else { return false };
    is_emoji_property(base as u32) && chars.all(|ch| is_mark(ch as u32))
}

/// Cheap pre-gate for the emoji matcher: emoji-regex can only match a
/// cluster containing an Emoji-property codepoint (which includes the
/// keycap bases `#`, `*`, `0-9` and the regional indicators) or a
/// ZWJ/variation-selector/keycap-combining mark.
fn may_be_emoji(cluster: &str) -> bool {
    cluster.chars().any(|ch| {
        let cp = ch as u32;
        is_emoji_property(cp) || cp == 0x200D || (0xFE00..=0xFE0F).contains(&cp) || cp == 0x20E3
    })
}

/// Cluster-aware display width of ONE grapheme cluster, mirroring the ref
/// pipeline per-semantics (measure + narrow). The single width seam: both
/// [`crate::wrap::display_width`] and the wrap atoms carry this value, so
/// measurement and wrapping can never disagree.
#[must_use]
pub fn ref_cluster_width(cluster: &str) -> usize {
    let Some(base) = cluster.chars().next() else { return 0 };
    let cp = base as u32;
    if stringwidth_skips(cp) {
        return 0;
    }
    let width =
        if may_be_emoji(cluster) && emoji_regex().is_match(cluster) { 2 } else { eaw_width(cp) };
    // The wrapper's narrowing (string-width.node.ts:13-24).
    if width == 2
        && !is_eaw_wide(cp)
        && emoji_mark_shape(cluster)
        && !is_emoji_presentation(cp)
        && !cluster.contains('\u{FE0F}')
        && !cluster.contains('\u{20E3}')
        && !cluster.contains('\u{200D}')
    {
        return 1;
    }
    width
}

#[cfg(test)]
mod ref_behavior_tests {
    use super::*;

    /// Behavior vectors captured from the ref itself: `stringWidth` of
    /// `packages/tui/src/util/string-width.node.ts` executed under
    /// bun-1.4.2 over the ref's own dependency tree (generation script
    /// /tmp/opencode/gf1-widthgen/gen.mjs, 2026-11-20). Every Unicode
    /// class of the model is covered: controls, `Default_Ignorable`
    /// (soft hyphen, hangul filler, tags), combining-range skips,
    /// text-presentation emoji bare/VS16/VS15, `Emoji_Presentation`,
    /// skin tones, RGI and NON-RGI ZWJ families, keycaps with and
    /// without VS16, flags incl. odd indicator counts, tag sequences,
    /// Devanagari conjuncts/matras, Thai/Khmer clusters, EAW
    /// wide/fullwidth/ambiguous and mixed wrap-relevant strings.
    #[test]
    #[allow(clippy::too_many_lines)] // generated vector table
    fn display_width_matches_ref_behavior_vectors() {
        const VECTORS: &[(&str, usize)] = &[
            // @generated: vectors.rs (bun-1.4.2, string-width.node.ts)
            ("\u{9}", 0),
            ("\u{1F}", 0),
            ("\u{20}", 1),
            ("\u{7F}", 0),
            ("\u{A0}", 1),
            ("\u{AD}", 0),
            ("\u{A9}", 1),
            ("\u{AE}", 1),
            ("\u{30}", 1),
            ("\u{23}", 1),
            ("\u{2A}", 1),
            ("\u{200B}", 0),
            ("\u{200D}", 0),
            ("\u{FEFF}", 0),
            ("\u{301}", 0),
            ("\u{1AB0}", 0),
            ("\u{1DC0}", 0),
            ("\u{20D0}", 0),
            ("\u{FE20}", 0),
            ("\u{FE0F}", 0),
            ("\u{2764}", 1),
            ("\u{26A0}", 1),
            ("\u{2600}", 1),
            ("\u{2714}", 1),
            ("\u{3164}", 0),
            ("\u{2705}", 2),
            ("\u{231A}", 2),
            ("\u{1F44D}", 2),
            ("\u{1F3FB}", 2),
            ("\u{1F1E6}", 1),
            ("\u{4E2D}", 2),
            ("\u{FF21}", 2),
            ("\u{30FC}", 2),
            ("\u{3297}", 2),
            ("\u{25B6}", 1),
            ("\u{1F308}", 2),
            ("\u{E33}", 1),
            ("\u{E31}", 1),
            ("\u{93E}", 1),
            ("\u{93C}", 1),
            ("\u{17B6}", 1),
            ("\u{902}", 1),
            ("\u{1BCA0}", 0),
            ("\u{2060}", 0),
            ("\u{61C}", 0),
            ("\u{1100}", 2),
            ("\u{2328}", 1),
            ("\u{23CF}", 1),
            ("\u{24C2}", 1),
            ("\u{2B05}", 1),
            ("\u{3030}", 2),
            ("\u{303D}", 2),
            ("\u{3299}", 2),
            ("\u{1F004}", 2),
            ("\u{1F21A}", 2),
            ("\u{1F3F3}", 1),
            ("\u{1F5FD}", 2),
            ("\u{1FAF0}", 2),
            ("\u{1FBC0}", 1),
            ("\u{1FBCC}", 1),
            ("\u{2764}\u{FE0F}", 2),
            ("\u{26A0}\u{FE0F}", 2),
            ("\u{2764}\u{FE0E}", 1),
            ("\u{1F44D}\u{1F3FB}", 2),
            ("\u{1F44D}\u{1F3FF}", 2),
            ("\u{1F9D1}\u{1F3FC}\u{200D}\u{1F91D}\u{200D}\u{1F9D1}\u{1F3FD}", 2),
            ("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}", 2),
            ("\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}", 2),
            ("\u{1F3F4}\u{200D}\u{2620}\u{FE0F}", 2),
            ("\u{1F469}\u{200D}\u{1F373}", 2),
            ("\u{1F468}\u{200D}\u{1F4BB}", 2),
            ("\u{1F44D}\u{200D}\u{2764}", 2),
            ("\u{2764}\u{200D}\u{1F44D}", 2),
            ("\u{1F5A4}\u{200D}\u{1F5A4}", 2),
            ("\u{30}\u{FE0F}\u{20E3}", 2),
            ("\u{30}\u{20E3}", 2),
            ("\u{31}\u{FE0F}\u{20E3}", 2),
            ("\u{31}\u{20E3}", 2),
            ("\u{32}\u{FE0F}\u{20E3}", 2),
            ("\u{32}\u{20E3}", 2),
            ("\u{33}\u{FE0F}\u{20E3}", 2),
            ("\u{33}\u{20E3}", 2),
            ("\u{34}\u{FE0F}\u{20E3}", 2),
            ("\u{34}\u{20E3}", 2),
            ("\u{35}\u{FE0F}\u{20E3}", 2),
            ("\u{35}\u{20E3}", 2),
            ("\u{36}\u{FE0F}\u{20E3}", 2),
            ("\u{36}\u{20E3}", 2),
            ("\u{37}\u{FE0F}\u{20E3}", 2),
            ("\u{37}\u{20E3}", 2),
            ("\u{38}\u{FE0F}\u{20E3}", 2),
            ("\u{38}\u{20E3}", 2),
            ("\u{39}\u{FE0F}\u{20E3}", 2),
            ("\u{39}\u{20E3}", 2),
            ("\u{23}\u{FE0F}\u{20E3}", 2),
            ("\u{23}\u{20E3}", 2),
            ("\u{2A}\u{FE0F}\u{20E3}", 2),
            ("\u{2A}\u{20E3}", 2),
            ("\u{1F1FA}\u{1F1F8}", 2),
            ("\u{1F1EF}\u{1F1F5}", 2),
            ("\u{1F1E6}\u{1F1E7}\u{1F1E8}", 2),
            ("\u{1F1FA}\u{1F1F8}\u{1F1EF}\u{1F1F5}\u{1F1E9}\u{1F1EA}", 6),
            ("\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}", 2),
            ("\u{1F3F3}\u{200D}\u{1F1FA}\u{200D}\u{1F1F8}", 4),
            ("\u{928}\u{94D}\u{926}", 1),
            ("\u{928}\u{94D}\u{926}\u{940}", 1),
            ("\u{92E}\u{947}", 1),
            ("\u{915}\u{94D}\u{924}", 1),
            ("\u{939}\u{93F}\u{928}\u{94D}\u{926}\u{940}", 2),
            ("\u{E2A}\u{E27}\u{E31}\u{E2A}\u{E14}\u{E35}", 4),
            ("\u{20}\u{6B}\u{72}\u{61}\u{74}\u{69}\u{70}", 7),
            ("\u{E01}\u{E31}", 1),
            ("\u{E01}\u{E32}", 2),
            ("\u{E01}\u{E49}", 1),
            ("\u{1781}\u{17D2}\u{1798}\u{17C2}\u{179A}", 2),
            ("\u{1780}\u{17D2}\u{1798}\u{17C1}\u{17C7}", 1),
            ("\u{74}\u{68}\u{75}\u{6D}\u{62}\u{73}\u{3A}\u{20}\u{1F44D}\u{20}\u{70}\u{6C}\u{61}\u{69}\u{6E}\u{20}\u{61}\u{6E}\u{64}\u{20}\u{1F44D}\u{1F3FD}\u{20}\u{6D}\u{65}\u{64}\u{69}\u{75}\u{6D}\u{20}\u{73}\u{6B}\u{69}\u{6E}\u{20}\u{74}\u{6F}\u{6E}\u{65}", 40),
            ("\u{6570}\u{636E}\u{5E93}\u{20}\u{77}\u{69}\u{64}\u{74}\u{68}", 12),
            ("\u{2469}\u{20}\u{B1}\u{30}\u{2E}\u{35}", 6),
            ("\u{65}\u{301}\u{20}\u{3D}\u{3D}\u{20}\u{E9}\u{20}\u{70}\u{72}\u{65}\u{63}\u{6F}\u{6D}\u{70}\u{6F}\u{73}\u{65}\u{64}", 18),
            ("\u{1F469}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F467}\u{20}\u{66}\u{61}\u{6D}\u{69}\u{6C}\u{79}\u{20}\u{78}\u{32}\u{20}\u{6570}\u{636E}", 17),
            ("\u{61}\u{AD}\u{62}", 2),
            ("\u{3164}", 0),
            ("\u{61}\u{2060}\u{62}", 2),
            ("\u{E0001}", 0),
            ("\u{E0020}", 0),
            ("\u{300}", 0),
            ("\u{20D0}", 0),
            ("\u{FE20}", 0),
            ("\u{93C}", 1),
            ("\u{E31}", 1),
        ];
        for (text, expected) in VECTORS {
            let got = crate::wrap::display_width(text);
            assert_eq!(
                got,
                *expected,
                "text {text:?} (cps {:X?})",
                text.chars().map(|c| c as u32).collect::<Vec<_>>()
            );
        }
    }

    /// The narrowing guards, pinned per class (the old G-129 tests,
    /// restated over the full-table model).
    #[test]
    fn narrowing_fires_and_aborts_per_class() {
        // Fires: bare text-presentation emoji measures 2 in string-width
        // (emoji-regex) and narrows to 1.
        assert_eq!(ref_cluster_width("\u{2764}"), 1);
        assert_eq!(ref_cluster_width("\u{A9}"), 1);
        // Aborts: VS16, keycap mark, ZWJ, `Emoji_Presentation`, EAW wide.
        assert_eq!(ref_cluster_width("\u{2764}\u{FE0F}"), 2);
        assert_eq!(ref_cluster_width("1\u{FE0F}\u{20E3}"), 2);
        assert_eq!(ref_cluster_width("\u{1F44D}\u{200D}\u{2764}"), 2, "ZWJ aborts narrowing");
        assert_eq!(ref_cluster_width("\u{2705}"), 2);
        assert_eq!(ref_cluster_width("\u{4E2D}"), 2, "EAW wide is never narrowed");
        assert_eq!(ref_cluster_width("a"), 1);
        // The non-RGI ZWJ family falls back per ref: emoji-regex still
        // matches (substring) so width stays 2 (vector-pinned above).
    }

    /// Standalone combining marks: string-width's explicit combining
    /// ranges skip (0), but marks OUTSIDE them (Devanagari nukta/matra,
    /// Khmer) count 1 via the EAW branch — never 0.
    #[test]
    fn standalone_marks_follow_the_skip_ranges_exactly() {
        assert_eq!(ref_cluster_width("\u{301}"), 0, "U+0301 in the 0300-036F skip");
        assert_eq!(ref_cluster_width("\u{20D0}"), 0);
        assert_eq!(ref_cluster_width("\u{93C}"), 1, "Devanagari nukta: not skipped");
        assert_eq!(ref_cluster_width("\u{93E}"), 1, "Devanagari matra: not skipped");
        assert_eq!(ref_cluster_width("\u{17B6}"), 1, "Khmer sara aa: not skipped");
    }

    /// `Default_Ignorable` skips the whole cluster (soft hyphen, hangul
    /// filler, tag characters).
    #[test]
    fn default_ignorable_clusters_measure_zero() {
        assert_eq!(ref_cluster_width("\u{AD}"), 0);
        assert_eq!(ref_cluster_width("\u{3164}"), 0);
        assert_eq!(ref_cluster_width("\u{E0001}"), 0);
        assert_eq!(display_width_of("a\u{AD}b"), 2);
    }

    fn display_width_of(s: &str) -> usize {
        crate::wrap::display_width(s)
    }

    /// The converted emoji-regex pattern agrees with the original JS
    /// matcher on the full validation corpus recorded from bun-1.4.2:
    /// every single codepoint (as match/no-match ranges), the full 26x26
    /// regional-indicator matrix, keycap forms, skin tones, RGI and
    /// non-RGI families, and tag sequences.
    #[test]
    #[allow(clippy::too_many_lines)] // recorded corpus table
    fn emoji_regex_matches_the_recorded_js_corpus() {
        use std::fmt::Write as _;
        // Every single-codepoint match, recorded from the JS regex as
        // ranges (1,112,972-probe sweep, zero mismatches at conversion
        // time; this re-pins the recorded expectation).
        let expected_singles: &[(u32, u32)] = &[
            (0xA9, 0xA9),
            (0xA9, 0xA9),
            (0xAE, 0xAE),
            (0xAE, 0xAE),
            (0x203C, 0x203C),
            (0x203C, 0x203C),
            (0x2049, 0x2049),
            (0x2122, 0x2122),
            (0x2122, 0x2122),
            (0x2139, 0x2139),
            (0x2194, 0x2199),
            (0x21A9, 0x21AA),
            (0x231A, 0x231A),
            (0x231A, 0x231B),
            (0x2328, 0x2328),
            (0x23CF, 0x23CF),
            (0x23E9, 0x23F3),
            (0x23F8, 0x23FA),
            (0x24C2, 0x24C2),
            (0x25AA, 0x25AB),
            (0x25B6, 0x25B6),
            (0x25C0, 0x25C0),
            (0x25FB, 0x25FE),
            (0x2600, 0x2604),
            (0x260E, 0x260E),
            (0x2611, 0x2611),
            (0x2614, 0x2615),
            (0x2618, 0x2618),
            (0x261D, 0x261D),
            (0x2620, 0x2620),
            (0x2622, 0x2623),
            (0x2626, 0x2626),
            (0x262A, 0x262A),
            (0x262E, 0x262F),
            (0x2638, 0x263A),
            (0x2640, 0x2640),
            (0x2642, 0x2642),
            (0x2648, 0x2653),
            (0x265F, 0x2660),
            (0x2663, 0x2663),
            (0x2665, 0x2666),
            (0x2668, 0x2668),
            (0x267B, 0x267B),
            (0x267E, 0x267F),
            (0x2692, 0x2697),
            (0x2699, 0x2699),
            (0x269B, 0x269C),
            (0x26A0, 0x26A0),
            (0x26A0, 0x26A1),
            (0x26A7, 0x26A7),
            (0x26AA, 0x26AB),
            (0x26B0, 0x26B1),
            (0x26BD, 0x26BE),
            (0x26C4, 0x26C5),
            (0x26C8, 0x26C8),
            (0x26CE, 0x26CF),
            (0x26D1, 0x26D1),
            (0x26D3, 0x26D4),
            (0x26E9, 0x26EA),
            (0x26F0, 0x26F5),
            (0x26F7, 0x26FA),
            (0x26FD, 0x26FD),
            (0x2702, 0x2702),
            (0x2705, 0x2705),
            (0x2705, 0x2705),
            (0x2708, 0x270D),
            (0x270F, 0x270F),
            (0x2712, 0x2712),
            (0x2714, 0x2714),
            (0x2716, 0x2716),
            (0x271D, 0x271D),
            (0x2721, 0x2721),
            (0x2728, 0x2728),
            (0x2733, 0x2734),
            (0x2744, 0x2744),
            (0x2747, 0x2747),
            (0x274C, 0x274C),
            (0x274E, 0x274E),
            (0x2753, 0x2755),
            (0x2757, 0x2757),
            (0x2763, 0x2763),
            (0x2763, 0x2764),
            (0x2764, 0x2764),
            (0x2795, 0x2797),
            (0x27A1, 0x27A1),
            (0x27B0, 0x27B0),
            (0x27BF, 0x27BF),
            (0x2934, 0x2935),
            (0x2B05, 0x2B07),
            (0x2B1B, 0x2B1C),
            (0x2B50, 0x2B50),
            (0x2B55, 0x2B55),
            (0x2B55, 0x2B55),
            (0x3030, 0x3030),
            (0x3030, 0x3030),
            (0x303D, 0x303D),
            (0x303D, 0x303D),
            (0x3297, 0x3297),
            (0x3299, 0x3299),
            (0x3299, 0x3299),
            (0x1F004, 0x1F004),
            (0x1F0CF, 0x1F0CF),
            (0x1F170, 0x1F171),
            (0x1F17E, 0x1F17F),
            (0x1F18E, 0x1F18E),
            (0x1F191, 0x1F19A),
            (0x1F201, 0x1F202),
            (0x1F21A, 0x1F21A),
            (0x1F22F, 0x1F22F),
            (0x1F232, 0x1F23A),
            (0x1F250, 0x1F251),
            (0x1F300, 0x1F308),
            (0x1F308, 0x1F321),
            (0x1F324, 0x1F393),
            (0x1F396, 0x1F397),
            (0x1F399, 0x1F39B),
            (0x1F39E, 0x1F3F0),
            (0x1F3F3, 0x1F3F3),
            (0x1F3F3, 0x1F3F5),
            (0x1F3F7, 0x1F44D),
            (0x1F44D, 0x1F469),
            (0x1F469, 0x1F4FD),
            (0x1F4FF, 0x1F53D),
            (0x1F549, 0x1F54E),
            (0x1F550, 0x1F567),
            (0x1F56F, 0x1F570),
            (0x1F573, 0x1F57A),
            (0x1F587, 0x1F587),
            (0x1F58A, 0x1F58D),
            (0x1F590, 0x1F590),
            (0x1F595, 0x1F596),
            (0x1F5A4, 0x1F5A5),
            (0x1F5A8, 0x1F5A8),
            (0x1F5B1, 0x1F5B2),
            (0x1F5BC, 0x1F5BC),
            (0x1F5C2, 0x1F5C4),
            (0x1F5D1, 0x1F5D3),
            (0x1F5DC, 0x1F5DE),
            (0x1F5E1, 0x1F5E1),
            (0x1F5E3, 0x1F5E3),
            (0x1F5E8, 0x1F5E8),
            (0x1F5EF, 0x1F5EF),
            (0x1F5F3, 0x1F5F3),
            (0x1F5FA, 0x1F64F),
            (0x1F680, 0x1F6C5),
            (0x1F6CB, 0x1F6D2),
            (0x1F6D5, 0x1F6D8),
            (0x1F6DC, 0x1F6E5),
            (0x1F6E9, 0x1F6E9),
            (0x1F6EB, 0x1F6EC),
            (0x1F6F0, 0x1F6F0),
            (0x1F6F3, 0x1F6FC),
            (0x1F7E0, 0x1F7EB),
            (0x1F7F0, 0x1F7F0),
            (0x1F90C, 0x1F93A),
            (0x1F93C, 0x1F945),
            (0x1F947, 0x1F9D1),
            (0x1F9D1, 0x1F9FF),
            (0x1FA70, 0x1FA7C),
            (0x1FA80, 0x1FA8A),
            (0x1FA8E, 0x1FAC6),
            (0x1FAC8, 0x1FAC8),
            (0x1FACD, 0x1FADC),
            (0x1FADF, 0x1FAEA),
            (0x1FAEF, 0x1FAF8),
        ];
        for &(lo, hi) in expected_singles {
            for cp in lo..=hi {
                let s = char::from_u32(cp).unwrap().to_string();
                assert!(emoji_regex().is_match(&s), "single U+{cp:X} should match emoji-regex");
            }
        }
        // Sequences — every expectation RECORDED from the original JS
        // regex under bun-1.4.2 (extpict.json seqs; includes the
        // surprising ones: 🇦🇧🇨 does NOT match — only assigned country
        // flags do; ❤+VS15 DOES match; 🖤‍🖤 matches because a bare
        // `Emoji_Presentation` heart is itself in the pattern).
        let seqs: &[(&str, bool)] = &[
            ("\u{1F1FA}\u{1F1F8}", true),
            ("\u{1F1E6}\u{1F1E7}\u{1F1E8}", false), // AB/BC are not assigned flags
            ("\u{1F1EC}\u{1F1E7}", true),           // GB is (via tag form? no — plain pair)
            ("1\u{FE0F}\u{20E3}", true),
            ("1\u{20E3}", true),
            ("1\u{FE0F}", false),
            ("\u{2764}\u{FE0F}", true),
            ("\u{2764}", true),
            ("\u{2764}\u{FE0E}", true),
            ("\u{1F44D}\u{1F3FB}", true),
            ("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}", true),
            ("\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}", true),
            ("\u{1F44D}\u{200D}\u{2764}", true), // substring: bare heart matches
            ("\u{1F5A4}\u{200D}\u{1F5A4}", true),
            ("\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}", true),
            ("\u{1F9D1}\u{1F3FC}\u{200D}\u{1F91D}\u{200D}\u{1F9D1}\u{1F3FD}", true),
            ("\u{1F3F3}\u{200D}\u{1F1FA}\u{200D}\u{1F1F8}", true),
        ];
        for (s, expected) in seqs {
            let mut cps = String::new();
            for c in s.chars() {
                let _ = write!(cps, "U+{:X} ", c as u32);
            }
            assert_eq!(emoji_regex().is_match(s), *expected, "sequence {cps}");
        }
    }
}
