//! Word-wrap for text leaves — the production home of the wave-7.0 spike
//! algorithm, extended with the **single seam** the gate-8 layout
//! instruction requires: wrap opportunities and minimum-content
//! measurement both derive from [`is_break_after`], so a leaf's
//! shrink-to-fit minimum can never disagree with how it actually wraps.
//!
//! Break opportunities (observed against the golden corpus): after a
//! space (the breaking space is consumed), after `.` (UAX-14-style:
//! `data.*dir` wraps as `data.` / `*dir`), and after every CJK
//! ideograph (breaks-anywhere ⇒ for CJK text the "longest word" is a
//! single grapheme). `|` and `/` are NOT opportunities. Greedy fill:
//! pack characters until one overflows, break at the last legal
//! opportunity on the line, hard-break when none exists.
//!
//! [`min_content_width`] reports the longest run between opportunities —
//! for CJK text that is the widest single grapheme (2 cells for wide
//! chars), which is exactly the width [`wrap_label`] can hard-break to.
//! A full port of the renderer's line-break algorithm is future work
//! (see the wave 7.0 report); this set reproduces every golden wrap.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Display width of a string in the renderer's "unicode" width method
/// (`string-width@7.2.0` bundled by `@opentui/core`,
/// `ambiguousIsNarrow=true`, emoji presentation = 2) — pinned by the
/// `width-edge-*` golden sidecars.
///
/// `unicode-width` 0.2 already covers emoji ZWJ sequences, skin-tone
/// modifiers, VS16, keycap clusters, flag pairs and non-spacing marks.
/// The remaining gap is Brahmic-derived scripts: **spacing combining
/// marks** (Mc) count as their own cell there and **virama conjuncts**
/// split into two, while the reference clusters them into one grapheme
/// each. [`merged_cells`] subtracts those merges.
#[must_use]
pub fn display_width(text: &str) -> usize {
    text.width().saturating_sub(merged_cells(text))
}

/// Cells the per-char sum overcounts vs grapheme clustering: one per
/// spacing combining mark, one per virama-joined consonant.
fn merged_cells(text: &str) -> usize {
    let mut merged = 0usize;
    let mut previous_virama = false;
    for ch in text.chars() {
        if is_spacing_combining(ch) {
            merged += ch.width().unwrap_or(1);
        } else if previous_virama && ch.width() == Some(1) {
            // Virama + consonant = one conjunct grapheme (न्द counts once).
            merged += 1;
        }
        previous_virama = is_virama(ch);
    }
    merged
}

/// Spacing combining marks (Unicode `Mc`) of the Brahmic scripts — the
/// marks `unicode-width` measures as a cell but the reference clusters
/// into the preceding base (मे = one cell). Approximate range table;
/// the `width-edge-*` sidecars pin the sampled scripts.
fn is_spacing_combining(ch: char) -> bool {
    let c = ch as u32;
    matches!(c,
        0x0903 | 0x093B | 0x093E..=0x0940 | 0x0949..=0x094C | 0x094F            // Devanagari
        | 0x0983 | 0x09BE..=0x09C0 | 0x09C7..=0x09C8 | 0x09CB..=0x09CC | 0x09D7 // Bengali
        | 0x0A03 | 0x0A70..=0x0A71 | 0x0A75                                      // Gurmukhi
        | 0x0A83 | 0x0ABE..=0x0AC0 | 0x0AC2..=0x0AC4 | 0x0AC7..=0x0AC8
        | 0x0ACB..=0x0ACC                                                       // Gujarati
        | 0x0B03 | 0x0B3E | 0x0B40 | 0x0B47..=0x0B48 | 0x0B4B..=0x0B4C
        | 0x0B56..=0x0B57                                                       // Oriya
        | 0x0B83 | 0x0BBE..=0x0BBF | 0x0BC1..=0x0BC2 | 0x0BC6..=0x0BC8
        | 0x0BCA..=0x0BCC | 0x0BD7                                              // Tamil
        | 0x0C03 | 0x0C3E..=0x0C40 | 0x0C46..=0x0C48 | 0x0C4A..=0x0C4C
        | 0x0C55..=0x0C56                                                       // Telugu
        | 0x0CBE..=0x0CBF | 0x0CC6 | 0x0CCC | 0x0CD5..=0x0CD6                    // Kannada
        | 0x0D03 | 0x0D3E..=0x0D40 | 0x0D46..=0x0D48 | 0x0D4A..=0x0D4C | 0x0D57 // Malayalam
        | 0x0D82..=0x0D83 | 0x0DCF..=0x0DD1 | 0x0DD8..=0x0DDF | 0x0DF2..=0x0DF3 // Sinhala
        | 0x0F3E..=0x0F3F | 0x0F7F                                              // Tibetan
        | 0x1031 | 0x103B..=0x103C | 0x1056..=0x1057 | 0x1084                   // Myanmar
        | 0x17B6 | 0x17BE..=0x17C5 | 0x17C7..=0x17C8                             // Khmer
        | 0x0EB1                                                               // Lao
    )
}

/// Sanskrit viramas: `virama + consonant` merges into one conjunct
/// grapheme (the reference counts one cell for न्द).
fn is_virama(ch: char) -> bool {
    matches!(
        ch as u32,
        0x094D | 0x09CD | 0x0A4D | 0x0ACD | 0x0B4D | 0x0C4D | 0x0CCD | 0x0D4D | 0x0DCA
    )
}

/// Characters after which a line break is legal. THE single seam shared
/// by wrapping ([`wrap_label`]) and measurement ([`min_content_width`]).
#[must_use]
pub fn is_break_after(ch: char) -> bool {
    ch == ' ' || ch == '.' || is_cjk(ch)
}

/// The CJK break class: ideographs, kana, Hangul, fullwidth forms and
/// extensions — every character in these ranges is its own break
/// opportunity (UAX-14 `ID`/`CJK` behavior, simplified).
fn is_cjk(ch: char) -> bool {
    let c = ch as u32;
    (0x1100..=0x115F).contains(&c)        // Hangul Jamo
        || (0x2E80..=0x303E).contains(&c)  // CJK Radicals..CJK Symbols
        || (0x3041..=0x33FF).contains(&c)  // Kana..CJK Compat
        || (0x3400..=0x4DBF).contains(&c)  // CJK Ext A
        || (0x4E00..=0x9FFF).contains(&c)  // CJK Unified
        || (0xA960..=0xA97F).contains(&c)  // Hangul Jamo Ext-A
        || (0xAC00..=0xD7A3).contains(&c)  // Hangul Syllables
        || (0xF900..=0xFAFF).contains(&c)  // CJK Compat Ideographs
        || (0xFE10..=0xFE19).contains(&c)  // Vertical forms
        || (0xFE30..=0xFE6F).contains(&c)  // CJK Compat Forms
        || (0xFF00..=0xFF60).contains(&c)  // Fullwidth Forms
        || (0xFFE0..=0xFFE6).contains(&c)  // Fullwidth Signs
        || (0x20000..=0x3FFFD).contains(&c) // CJK Ext B+
}

/// The CSS automatic minimum (min-content) of a text leaf: the display
/// width of the longest run between break opportunities. Break-anywhere
/// scripts (CJK) therefore report their widest grapheme — consistent
/// with [`wrap_label`], which can hard-break any run down to one
/// grapheme.
#[must_use]
pub fn min_content_width(text: &str) -> usize {
    let mut longest = 0usize;
    let mut current = 0usize;
    for ch in text.chars() {
        current += ch.width().unwrap_or(1);
        if is_break_after(ch) {
            if ch == ' ' {
                // The breaking space is consumed, not part of the word.
                current -= 1;
            }
            longest = longest.max(current);
            current = 0;
        }
    }
    longest.max(current).max(widest_grapheme(text))
}

/// The intrinsic single-line (max-content) width.
#[must_use]
pub fn max_content_width(text: &str) -> usize {
    display_width(text)
}

/// Longest prefix of `text` fitting `width` display cells
/// (`Locale.takeWidth` — `wrapMode="none"` truncation).
#[must_use]
pub fn take_width(text: &str, width: usize) -> String {
    let mut used = 0usize;
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        // Zero-width chars (marks, ZWJ) attach without consuming cells.
        let cost = crate::wrap::char_display_width(ch);
        if used + cost > width {
            break;
        }
        used += cost;
        end = index + ch.len_utf8();
    }
    text[..end].to_owned()
}

/// Display width of a single char (cluster-approximated: marks/ZWJ are 0
/// so they never consume a truncation slot on their own).
fn char_display_width(ch: char) -> usize {
    use unicode_width::UnicodeWidthChar;
    match ch.width() {
        Some(0) | None => 0,
        Some(width) => width,
    }
}

fn widest_grapheme(text: &str) -> usize {
    // Grapheme clusters are approximated by chars here (combining marks
    // measure as their own zero/narrow width — see the wave-7.0 report
    // for the full-cluster caveat).
    text.chars().map(|ch| ch.width().unwrap_or(1)).max().unwrap_or(0)
}

/// Wrap `label` into lines of at most `width` display cells.
#[must_use]
pub fn wrap_label(label: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line_start = 0usize;
    // Byte index just past a legal break point on the current line.
    let mut last_break: Option<usize> = None;
    let mut line_width = 0usize;
    let mut position = 0usize;
    while let Some(ch) = label[position..].chars().next() {
        let ch_width = ch.width().unwrap_or(1);
        let index = position;
        if line_width + ch_width > width {
            if ch == ' ' {
                // The overflowing space is itself the furthest break point
                // and is consumed by the break.
                lines.push(label[line_start..index].to_string());
                line_start = skip_spaces(label, index);
                last_break = None;
                line_width = 0;
                position = line_start;
                continue;
            }
            let break_at = match last_break {
                Some(break_index) if break_index > line_start => break_index,
                _ => index,
            };
            let mut line_end = break_at;
            if label[line_start..line_end].ends_with(' ') {
                line_end -= 1;
            }
            lines.push(label[line_start..line_end].to_string());
            line_start = skip_spaces(label, line_end);
            last_break = None;
            // The overflowing character opens the next line together with
            // any content already committed past the break point.
            line_width = display_width(&label[line_start..index + ch.len_utf8()]);
            position = index + ch.len_utf8();
            continue;
        }
        line_width += ch_width;
        if is_break_after(ch) {
            last_break = Some(index + ch.len_utf8());
        }
        position = index + ch.len_utf8();
    }
    let tail = label[line_start..].trim_end_matches(' ').to_string();
    lines.push(tail);
    lines
}

fn skip_spaces(label: &str, mut index: usize) -> usize {
    while label[index..].starts_with(' ') {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_on_spaces_and_drops_breaking_space() {
        assert_eq!(wrap_label("aaa bb ccc", 5), vec!["aaa", "bb", "ccc"]);
    }

    #[test]
    fn keeps_fitting_words_together() {
        assert_eq!(wrap_label("aa bb cc", 5), vec!["aa bb", "cc"]);
    }

    #[test]
    fn hard_breaks_overlong_tokens() {
        assert_eq!(wrap_label("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn breaks_at_dots_when_no_space_fits() {
        // `.` is an opportunity; `|` is not: lines break at the last legal
        // opportunity (UAX-14-style greedy), then hard-fill the remainder.
        assert_eq!(wrap_label("a.b|c|d", 4), vec!["a.", "b|c|", "d"]);
        assert_eq!(
            wrap_label("Global\\.Path\\.data|data", 10),
            vec!["Global\\.", "Path\\.", "data|data"]
        );
    }

    #[test]
    fn reference_labels_wrap_like_the_golden() {
        let label = r#"Grep "OPENCODE.*DB|database|sqlite|drizzle|dev.*db|data.*dir|xdg|APPDATA" in packages/opencode/src (151 matches)"#;
        assert_eq!(
            wrap_label(label, 67),
            vec![
                r#"Grep "OPENCODE.*DB|database|sqlite|drizzle|dev.*db|data."#,
                r#"*dir|xdg|APPDATA" in packages/opencode/src (151 matches)"#,
            ]
        );
    }

    #[test]
    fn min_content_matches_wrap_opportunities() {
        // Longest run between opportunities: "data." keeps its dot; `|` is
        // not an opportunity so "dir|verylongpath" is ONE run.
        assert_eq!(min_content_width("data.dir|verylongpath"), "dir|verylongpath".len());
        assert_eq!(min_content_width("aaa bb"), 3);
        assert_eq!(min_content_width("a.b.c"), 2);
    }

    #[test]
    fn cjk_breaks_anywhere_and_measures_one_grapheme() {
        // Gate-8 layout instruction: CJK breaks-anywhere ⇒ longest word is
        // one grapheme (2 display cells for wide chars) — never the whole
        // line, never 0.
        let text = "日本語テキスト";
        assert_eq!(min_content_width(text), 2);
        assert_eq!(wrap_label(text, 4), vec!["日本", "語テ", "キス", "ト"]);
        assert_eq!(wrap_label(text, 6), vec!["日本語", "テキス", "ト"]);
        // Mixed CJK/Latin: no opportunity between `config` and の (only
        // AFTER each CJK char), so the run is "configの" — 8 display
        // cells — the same seam wrapping hard-breaks below that.
        let mixed = "設定configの値";
        assert_eq!(min_content_width(mixed), 8);
        assert_eq!(min_content_width(mixed), display_width("configの"));
        assert_eq!(wrap_label(mixed, 4), vec!["設定", "conf", "igの", "値"]);
    }

    #[test]
    fn cjk_min_content_enables_shrink_to_fit_consistency() {
        // The consistency property the gate demands: whatever width the
        // flex solver lands on (≥ min-content), wrapping succeeds without
        // stalling — every line fits, and content is never dropped.
        let text = "データベースの設定";
        let minimum = min_content_width(text);
        assert_eq!(minimum, 2);
        for width in minimum..=display_width(text) {
            let lines = wrap_label(text, width);
            assert!(
                lines.iter().all(|line| display_width(line) <= width),
                "width {width}: {lines:?}"
            );
            assert_eq!(
                lines.iter().map(|line| line.chars().count()).sum::<usize>(),
                text.chars().count(),
                "no content dropped at {width}"
            );
        }
    }

    /// The `width-edge-*` golden sidecars pin `display_width` to the
    /// reference's own measurements (`string-width@7.2.0` bundled by
    /// `@opentui/core`, ambiguousIsNarrow, emoji presentation = 2):
    /// CJK/ambiguous/box-drawing, emoji ZWJ/modifier/VS16/keycap/flags,
    /// combining sequences incl. Indic conjuncts.
    #[cfg(test)]
    mod width_sidecars {
        use super::*;
        use parity::tui_frame as pf;

        fn goldens() -> std::path::PathBuf {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/parity/goldens/tui-frames")
        }

        #[test]
        fn display_width_matches_measured_sidecars() {
            for id in ["width-edge-cjk", "width-edge-emoji", "width-edge-combining"] {
                let sidecar = pf::load_widths_sidecar(&goldens(), id)
                    .unwrap_or_else(|error| panic!("sidecar {id}: {error}"));
                assert!(!sidecar.samples.is_empty());
                for sample in &sidecar.samples {
                    assert_eq!(
                        display_width(&sample.text),
                        sample.width,
                        "{id}: {:?}",
                        sample.text
                    );
                }
            }
        }

        #[test]
        fn anchors_of_each_width_class() {
            // ZWJ sequences collapse to one emoji cell.
            assert_eq!(display_width("👨‍👩‍👧‍👦"), 2);
            // Skin-tone modifiers are absorbed.
            assert_eq!(display_width("👍🏽"), 2);
            // VS16 + ZWJ (rainbow flag) is one cell.
            assert_eq!(display_width("🏳️‍🌈"), 2);
            // Keycap clusters are 2.
            assert_eq!(display_width("1️⃣"), 2);
            // Regional indicators pair into flags.
            assert_eq!(display_width("🇺🇸🇯🇵🇩🇪"), 6);
            // NFD and precomposed agree.
            assert_eq!(display_width("é"), display_width("e\u{301}"));
            // Indic conjuncts: न्द is one grapheme.
            assert_eq!(display_width("न्द"), 1);
            assert_eq!(display_width("मे"), 1);
            // Ambiguous width → narrow.
            assert_eq!(display_width("⑩"), 1);
            assert_eq!(display_width("±0.5"), 4);
            // CJK stays wide.
            assert_eq!(display_width("数据库"), 6);
        }
    }
}
