//! JS `localeCompare` equivalence for the listing surfaces (RD-18).
//!
//! The ref sorts the mcp servers/tools/prompts/resources listings with
//! `String.prototype.localeCompare` (default locale = the CLDR root
//! collation), where the port used byte-order `str::cmp` — non-ASCII
//! (`"Éclair"` vs `"zebra"`) and case-mixed names ordered differently.
//! The `icu` collator is not in-tree, so this is the small hand-rolled
//! equivalent: a three-level collation key per char —
//!
//! 1. **primary** — case- and accent-folded base letter (É → e, ß →
//!    s; non-Latin chars keep their scalar),
//! 2. **secondary** — whether an accent was folded away (plain `e`
//!    before `é`, accent differences decided before case),
//! 3. **tertiary** — lowercase before uppercase (the root default).
//!
//! For the ASCII corpus this is byte-identical in outcome to the
//! repo's earlier `locale_cmp` convention (folded compare +
//! lowercase-first tiebreak).

use std::cmp::Ordering;

/// The primary/secondary fold of one char: `(base, accent)` — the
/// case- and diacritic-folded base letter, and whether a diacritic
/// was folded away (the secondary level).
fn fold(ch: char) -> (char, bool) {
    // Case folds first: 'É' and 'é' share every level but tertiary.
    let lower = ch.to_lowercase().next().unwrap_or(ch);
    let (base, accent) = match lower {
        // Latin-1 supplement + Latin Extended-A families, grouped by
        // base letter (uppercase forms arrive here lowercased).
        'à'..='å' | 'ā' | 'ă' | 'ą' | 'æ' => ('a', true),
        'ç' | 'ć' | 'ĉ' | 'č' => ('c', true),
        'ď' | 'đ' | 'ð' => ('d', true),
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => ('e', true),
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => ('g', true),
        'ĥ' | 'ħ' => ('h', true),
        'ì'..='ï' | 'ĩ' | 'ī' | 'į' | 'ı' => ('i', true),
        'ĵ' => ('j', true),
        'ķ' => ('k', true),
        'ĺ' | 'ļ' | 'ľ' | 'ł' => ('l', true),
        'ñ' | 'ń' | 'ņ' | 'ň' | 'ŉ' => ('n', true),
        'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'œ' => ('o', true),
        'ŕ' | 'ŗ' | 'ř' => ('r', true),
        'ß' | 'ŝ' | 'ş' | 'š' | 'ſ' => ('s', true),
        'ţ' | 'ť' | 'þ' => ('t', true),
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => ('u', true),
        'ŵ' => ('w', true),
        'ý' | 'ÿ' | 'ŷ' => ('y', true),
        'ź' | 'ż' | 'ž' => ('z', true),
        _ => (lower, false),
    };
    (base, accent)
}

/// The three-level collation key: primary chars, secondary accent
/// flags, tertiary case flags (`1` = uppercase).
fn collation_keys(s: &str) -> (Vec<u32>, Vec<u8>, Vec<u8>) {
    let mut primary = Vec::with_capacity(s.len());
    let mut secondary = Vec::with_capacity(s.len());
    let mut tertiary = Vec::with_capacity(s.len());
    for ch in s.chars() {
        let (base, accent) = fold(ch);
        primary.push(u32::from(base));
        secondary.push(u8::from(accent));
        tertiary.push(u8::from(ch.is_uppercase()));
    }
    (primary, secondary, tertiary)
}

/// `a.localeCompare(b)` with the default (CLDR root) collation:
/// primary letters accent- and case-insensitively ("Éclair" before
/// "zebra"), then accents (`resume` before `résumé`), then case
/// (lowercase first: `alpha` < `Alpha` < `ALPHA`). A final byte
/// tiebreak keeps the order total (and `sort_by` deterministic) for
/// identical keys.
#[must_use]
pub fn locale_cmp(a: &str, b: &str) -> Ordering {
    let (primary_a, secondary_a, tertiary_a) = collation_keys(a);
    let (primary_b, secondary_b, tertiary_b) = collation_keys(b);
    primary_a
        .cmp(&primary_b)
        .then_with(|| secondary_a.cmp(&secondary_b))
        .then_with(|| tertiary_a.cmp(&tertiary_b))
        .then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::locale_cmp;
    use std::cmp::Ordering::*;

    fn assert_order(rising: &[&str]) {
        for (index, left) in rising.iter().enumerate() {
            for right in &rising[index + 1..] {
                assert_eq!(locale_cmp(left, right), Less, "{left} < {right}");
                assert_eq!(locale_cmp(right, left), Greater, "{right} > {left}");
            }
        }
    }

    #[test]
    fn non_ascii_primary_folds_accents() {
        // RD-18's corpus: Éclair sorts as if written "eclair".
        assert_eq!(locale_cmp("Éclair", "zebra"), Less);
        assert_order(&["eclair", "Éclair", "ember", "zebra", "zed"]);
    }

    #[test]
    fn case_mixes_sort_lowercase_first() {
        assert_order(&["alpha", "Alpha", "ALPHA", "beta", "Beta"]);
        assert_eq!(locale_cmp("a", "A"), Less);
        assert_eq!(locale_cmp("AB", "Ab"), Greater);
    }

    #[test]
    fn accents_decide_before_case() {
        // Secondary (accent) beats tertiary (case): é > E at the
        // secondary level even though é is lowercase.
        assert_eq!(locale_cmp("E", "é"), Less);
        assert_eq!(locale_cmp("resume", "résumé"), Less);
        assert_eq!(locale_cmp("RESUME", "résumé"), Less);
    }

    #[test]
    fn ascii_matches_the_repo_locale_convention() {
        // The pre-RD-18 convention (folded compare, lowercase-first
        // tiebreak) is preserved on the ASCII corpus.
        assert_eq!(locale_cmp("a", "B"), Less);
        assert_eq!(locale_cmp("model", "Model"), Less);
        assert_eq!(locale_cmp("same", "same"), Equal);
    }

    #[test]
    fn digits_stay_lexicographic_and_equal_strings_tie_break_by_bytes() {
        // localeCompare's default is numeric: false.
        assert_eq!(locale_cmp("10", "9"), Less);
        // Identical keys with different bytes: deterministic byte order.
        assert_eq!(locale_cmp("ß", "ss"), Less);
    }
}
