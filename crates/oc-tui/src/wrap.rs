//! Word-wrap for text leaves over ONE flattened text stream — the
//! production home of the wave-7.0 spike algorithm, extended with the
//! **single seam** the gate-8 layout instruction requires and the
//! **styled-segment stream** the gate-9 wave-0 mandate requires.
//!
//! # The flattened stream (styled-segment leaves)
//!
//! A text leaf is either a plain string or a list of [`StyledSpan`]s
//! (`Vec<(text, style)>`). Both flatten to the SAME stream of
//! [`Atom`]s — one **grapheme cluster** each, carrying its display
//! width and the style of the span it came from. Wrapping operates on
//! that stream, so a break opportunity landing **mid-span** works
//! exactly like a break between spans (the opentui text-children
//! seam): wrapping per-span would diverge at every wrap boundary that
//! falls inside a span.
//!
//! Clusters are the ref's grapheme rules on full tables (MF#16): joining
//! is a port of the ref's own `Intl.Segmenter` behavior as observed under
//! bun-1.4.2 (see [`flatten_spans`]) — marks/ZWJ/VS16 join their base,
//! skin tones absorb, regional indicators pair into flags, spacing marks
//! (Mc, full `\p{gc=Spacing_Mark}` sweep) attach, and `GB9c` conjuncts join
//! through the engine's exact linker/consonant sets (Khmer coeng and
//! Myanmar asat conjunct, Gurmukhi/Kannada/Sinhala/Tamil viramas do not —
//! the ref engine's observed behavior, [`crate::width_tables`]).
//! **A hard break therefore never splits a cluster** (gate-9 finding 7:
//! ZWJ families stay whole) because the wrap loop moves whole atoms.
//!
//! # Break opportunities (the single seam)
//!
//! Break opportunities (observed against the golden corpus): after a
//! space (a breaking space that **fits** stays at the end of the line —
//! the markdown goldens pin its styled trailing cell), after `.` and
//! `-` (UAX-14-style: `data.*dir` wraps as `data.` / `*dir`;
//! `bold-run-` wraps at hyphens), and after every CJK ideograph
//! (breaks-anywhere ⇒ for CJK text the "longest word" is a single
//! grapheme). `|` and `/` are NOT opportunities. Greedy fill: pack
//! atoms until one overflows, break at the last legal opportunity on
//! the line, hard-break when none exists.
//!
//! [`atoms_min_content_width`] reports the longest run between
//! opportunities — for CJK text that is the widest single grapheme (2
//! cells for wide chars), which is exactly the width
//! [`wrap_atoms`] can hard-break to. Measurement and wrapping share
//! [`break_after_atom`], so a leaf's shrink-to-fit minimum can never
//! disagree with how it actually wraps. A full port of the renderer's
//! line-break algorithm is future work (see the wave 7.0 report); this
//! set reproduces every golden wrap.

use crate::style::Style as PaintStyle;
use crate::width_model;
use crate::width_tables::{EXTENDED_PICTOGRAPHIC, INCB_CONSONANT, INCB_LINKER, SPACING_MARK};
use unicode_width::UnicodeWidthChar;

/// One styled run of a styled-segment text leaf: the span's text plus
/// the style applied to every cell it occupies. Reused as the run type
/// of placed lines ([`styled_runs`] re-assembles wrapped atoms into
/// runs of contiguous style at paint time).
#[derive(Debug, Clone, PartialEq)]
pub struct StyledSpan {
    pub text: String,
    pub style: PaintStyle,
    /// Hyperlink URL (lane KM, auditA #9): markdown chunks carrying
    /// `link: { url }` keep it through wrap/paint — the transcript's
    /// hit walk reads it to arm clickable links (`ui/link.tsx:27`).
    /// `None` on every non-markdown span.
    pub link: Option<String>,
}

/// One grapheme-cluster atom of the flattened text stream: the cluster
/// string, its cluster-aware display width, and the style of the span
/// the cluster came from. Wrapping, measurement and painting all move
/// whole atoms — the cluster-safety guarantee (finding 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Atom {
    pub text: String,
    pub width: usize,
    pub style: PaintStyle,
    /// The source span's hyperlink URL (see [`StyledSpan::link`]).
    pub link: Option<String>,
}

/// Display width of a string in the renderer's "unicode" width method —
/// the full-table port of the ref's `string-width.node.ts` (MF#16:
/// `string-width@7.2.0` measure + the wrapper's text-presentation emoji
/// narrowing, per grapheme cluster). See
/// [`width_model::ref_cluster_width`] for the per-semantics anchor and
/// [`width_model`] for the generated-table provenance; the
/// `ref_behavior_tests` module pins the model to behavior vectors
/// captured from the ref under bun-1.4.2.
#[must_use]
pub fn display_width(text: &str) -> usize {
    let mut total = 0usize;
    for_each_cluster(text, |cluster| total += width_model::ref_cluster_width(cluster));
    total
}

/// Cluster-aware display width of one grapheme cluster — the same
/// measure as [`display_width`], applied per cluster. Atoms carry this
/// value, so the sum of a flattened stream's atom widths equals the
/// display width of the concatenated text.
fn atom_width(cluster: &str) -> usize {
    width_model::ref_cluster_width(cluster)
}

// -- G-129 → MF#16: the width model moved to `width_model` ---------------
// The text-presentation emoji narrowing and its tables now live in
// [`crate::width_model`] as a full-property port of the ref's
// `string-width.node.ts` (the static `TEXT_EMOJI_BASE` block table is
// retired; `\p{Emoji}` / `\p{Emoji_Presentation}` /
// `\p{Default_Ignorable_Code_Point}` / EAW F/W are swept from the ref
// runtime). Its `ref_behavior_tests` module pins the per-class behavior.

/// Spacing combining marks (Unicode `Mc`, full
/// `\p{gc=Spacing_Mark}` sweep) — joins the preceding cluster (UAX-29
/// `GB9a`): मे = one cell in the ref.
fn is_spacing_combining(ch: char) -> bool {
    width_model::in_table(SPACING_MARK, ch as u32)
}

/// `GB9c` conjunct linkers as the REF's segmenter implements them
/// (derived by probing bun-1.4.2 `Intl.Segmenter`; see
/// [`crate::width_tables::INCB_LINKER`]): Devanagari/Bengali/Gujarati/
/// Oriya/Telugu/Malayalam viramas, Myanmar asat (1039), Khmer coeng
/// (17D2), Tai Tham (1A60). Gurmukhi 0A4D, Kannada 0CCD, Sinhala 0DCA
/// and Tamil 0BCD do NOT link in the ref's engine.
fn is_linker(ch: char) -> bool {
    width_model::in_table(INCB_LINKER, ch as u32)
}

/// `GB9c` conjunct consonants per the ref's segmenter
/// ([`crate::width_tables::INCB_CONSONANT`]): a linker joins the
/// FOLLOWING consonant into the cluster (न्द counts once); anything
/// else after a linker starts a new cluster, exactly like the ref.
fn is_incb_consonant(ch: char) -> bool {
    width_model::in_table(INCB_CONSONANT, ch as u32)
}

/// `\p{Extended_Pictographic}` — `GB11`: a ZWJ joins only a pictographic
/// codepoint into the cluster. Regional indicators are not joinable
/// across a ZWJ (the ref engine splits `🏳‍🇺‍🇸` into three clusters —
/// probed under bun-1.4.2), so the ZWJ join requires `ExtPict` AND not a
/// regional indicator.
fn is_extended_pictographic(ch: char) -> bool {
    width_model::in_table(EXTENDED_PICTOGRAPHIC, ch as u32)
}

/// Emoji skin-tone modifiers (U+1F3FB..=U+1F3FF): absorbed by the
/// preceding emoji cluster (👍🏽 = one cell of width 2).
fn is_skin_tone(ch: char) -> bool {
    matches!(ch as u32, 0x1F3FB..=0x1F3FF)
}

/// Regional indicators (U+1F1E6..=0x1F1FF): consecutive pairs form one
/// flag cluster (🇺🇸 = 2 cells), so the second indicator joins the
/// first and a third starts a new cluster.
fn is_regional_indicator(ch: char) -> bool {
    matches!(ch as u32, 0x1F1E6..=0x1F1FF)
}

/// G-452 — the reference's emoji emission shape (observed ref grid,
/// `w13-g-emoji` pair dump; native shaper semantics vendored in the
/// sister repo at
/// opentui-rust checkout `third_party/opentui/packages/native/src/utf8.zig:438-459`
/// plus grapheme packing): the ref terminal never receives the full
/// ZWJ family or the skin-tone modifier bytes. Per cluster:
///
/// * a cluster containing ZWJ emits **up to and including the first
///   ZWJ** — `👨‍👩‍👧‍👦` emits `👨‍`, `🏳️‍🌈` emits `🏳️‍` (the
///   ZWJ itself is kept, zero-width, per `utf8.zig:438` `cp == 0x200D
///   → 0`);
/// * skin-tone modifiers (`1F3FB..=1F3FF`) are dropped — `👍🏽`
///   emits `👍`;
/// * VS16 (`FE0F`) is KEPT and zero-width (the `❤️` cluster stays
///   `❤\u{fe0f}`);
/// * the cluster WIDTH is unchanged (family/head = 2, `❤️` = 2 via
///   the VS16 presentation upgrade), so when the emitted advance is
///   narrower than the width (the `❤️` case: advance 1, width 2) the
///   reference pads with the following blank cell — the observed
///   double space after `❤️` in the ref grid.
///
/// Applied at the single seam ([`flatten_spans`]) so wrap decisions
/// (width-driven) are untouched while paint/emit carry the ref byte
/// shape.
fn ref_emission_shape(cluster: &str) -> String {
    if cluster.contains('\u{200D}') {
        let mut out = String::with_capacity(cluster.len());
        for ch in cluster.chars() {
            out.push(ch);
            if ch == '\u{200D}' {
                break;
            }
        }
        out
    } else if cluster.chars().any(is_skin_tone) {
        cluster.chars().filter(|ch| !is_skin_tone(*ch)).collect()
    } else {
        cluster.to_owned()
    }
}

/// The ref's grapheme-cluster seam (UAX-29 as bun-1.4.2's
/// `Intl.Segmenter` implements it, on full property tables — see
/// [`crate::width_tables`]): zero-width chars (marks, ZWJ, VS16) join,
/// a ZWJ joins the following `Extended_Pictographic` codepoint (`GB11`
/// emoji sequences — regional indicators do NOT join across a ZWJ:
/// `🏳‍🇺‍🇸` is three clusters, exactly like the ref engine), skin
/// tones absorb, spacing marks attach (`GB9a`), the second regional
/// indicator of a pair joins (`GB12`/`GB13`), and a `GB9c` linker joins the
/// following `InCB` consonant (conjuncts) — the engine's own
/// linker/consonant sets, so Khmer ក្ម and Myanmar ကျ conjunct while
/// Gurmukhi/Kannada/Sinhala/Tamil viramas do not, exactly like the ref.
///
/// Single seam shared by [`flatten_spans`] (atoms) and
/// [`display_width`] (measurement), so a flattened stream's atom widths
/// always sum to the text's display width.
fn for_each_cluster(text: &str, mut emit: impl FnMut(&str)) {
    let mut cluster = String::new();
    let mut indicators = 0u8;
    let mut after_linker = false;
    let mut after_zwj = false;
    for ch in text.chars() {
        let joins = !cluster.is_empty()
            && ((after_zwj && is_extended_pictographic(ch) && !is_regional_indicator(ch))
                || ch.width() == Some(0)
                || is_skin_tone(ch)
                || is_spacing_combining(ch)
                || (is_regional_indicator(ch) && indicators == 1)
                || (after_linker && is_incb_consonant(ch)));
        if !joins {
            if !cluster.is_empty() {
                emit(&std::mem::take(&mut cluster));
            }
            indicators = 0;
        }
        cluster.push(ch);
        after_linker = is_linker(ch);
        after_zwj = ch == '\u{200D}';
        indicators = if is_regional_indicator(ch) { indicators.saturating_add(1) } else { 0 };
    }
    if !cluster.is_empty() {
        emit(&cluster);
    }
}

/// Flatten a plain string under one style (the plain-leaf case of
/// [`flatten_spans`]).
#[must_use]
pub fn flatten(text: &str, style: PaintStyle) -> Vec<Atom> {
    flatten_spans(&[StyledSpan { text: text.to_owned(), style, link: None }])
}

/// Flatten styled spans into ONE text stream of cluster atoms. The
/// stream is seam-agnostic: a span boundary is NOT a break opportunity
/// (wrapping treats concatenated spans as one text, like opentui text
/// children), and clusters never merge ACROSS spans — a span is a text
/// node, so a modifier that opens a span forms its own cluster.
#[must_use]
pub fn flatten_spans(spans: &[StyledSpan]) -> Vec<Atom> {
    let mut atoms: Vec<Atom> = Vec::new();
    for span in spans {
        let style = span.style;
        let link = span.link.clone();
        for_each_cluster(&span.text, |cluster| {
            let width = atom_width(cluster);
            atoms.push(Atom {
                text: ref_emission_shape(cluster),
                width,
                style,
                link: link.clone(),
            });
        });
    }
    atoms
}

/// Characters after which a line break is legal. THE single seam shared
/// by wrapping ([`wrap_atoms`]) and measurement
/// ([`atoms_min_content_width`]).
///
/// `-` joined the set with the markdown goldens (7.2 lane C): the
/// reference wraps `bold-run-deliberately-longer-…` after the last
/// hyphen that fits at BOTH pinned widths (markdown-frame@80/40), so
/// hyphens are break opportunities exactly like dots.
#[must_use]
pub fn is_break_after(ch: char) -> bool {
    ch == ' ' || ch == '.' || ch == '-' || is_cjk(ch)
}

/// The break decision for one atom: the seam above evaluated on the
/// cluster's final char (the base of the cluster — spaces, dots and
/// CJK ideographs are always cluster-final in practice).
fn break_after_atom(atom: &Atom) -> bool {
    atom.text.chars().last().is_some_and(is_break_after)
}

/// A breaking space atom (spaces are always single-char clusters).
fn is_space_atom(atom: &Atom) -> bool {
    atom.text == " "
}

/// G-467.1 — kinsoku (UAX-14's "cl/ex" classes, simplified): CJK
/// closing punctuation may not START a wrapped line. When a break
/// would land right before such a character, the break moves one
/// atom earlier (the trailing ideograph carries the punctuation's
/// lead — ref w13-g: the line breaks after `…中日韩文`, never
/// leaving `，` to open the next line).
fn is_forbidden_line_start(ch: char) -> bool {
    matches!(
        ch,
        '，' | '、' | '。' | '？' | '！' | '：' | '；' | '）' | '】' | '」' | '』' | '〉' | '》'
            | '〕' | '］' | '｝' | '…' | '・' | '～'
    )
}

/// Pull a line end back while the NEXT line would open on a
/// forbidden-start character (kinsoku) — never below one atom.
fn kinsoku_pull_back(atoms: &[Atom], line_start: usize, line_end: usize) -> usize {
    let mut end = line_end;
    while end > line_start + 1
        && atoms
            .get(end)
            .and_then(|next| next.text.chars().next())
            .is_some_and(is_forbidden_line_start)
        && !is_space_atom(&atoms[end - 1])
    {
        end -= 1;
    }
    end
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

/// The CSS automatic minimum (min-content) of a flattened stream: the
/// display width of the longest run of atoms between break
/// opportunities. Break-anywhere scripts (CJK) therefore report their
/// widest grapheme — consistent with [`wrap_atoms`], which can
/// hard-break any run down to one atom (never splitting a cluster).
#[must_use]
pub fn atoms_min_content_width(atoms: &[Atom]) -> usize {
    let mut longest = 0usize;
    let mut current = 0usize;
    for atom in atoms {
        current += atom.width;
        if break_after_atom(atom) {
            if is_space_atom(atom) {
                // The breaking space is consumed, not part of the word.
                current -= atom.width;
            }
            longest = longest.max(current);
            current = 0;
        }
    }
    longest.max(current).max(atoms.iter().map(|atom| atom.width).max().unwrap_or(0))
}

/// The intrinsic single-line (max-content) width of a flattened stream.
#[must_use]
pub fn atoms_max_content_width(atoms: &[Atom]) -> usize {
    atoms.iter().map(|atom| atom.width).sum()
}

/// Wrap a flattened stream into lines of at most `width` display
/// cells. THE wrap seam: same opportunities as measurement, same
/// cluster atoms — a wrap boundary may land anywhere (including
/// mid-span); only whole atoms ever move.
#[must_use]
pub fn wrap_atoms(atoms: &[Atom], width: usize) -> Vec<Vec<Atom>> {
    if width == 0 {
        return vec![Vec::new()];
    }
    let mut lines: Vec<Vec<Atom>> = Vec::new();
    let mut line_start = 0usize;
    // Atom index just past a legal break point on the current line.
    let mut last_break: Option<usize> = None;
    let mut line_width = 0usize;
    let mut position = 0usize;
    while position < atoms.len() {
        let atom = &atoms[position];
        let index = position;
        if line_width + atom.width > width {
            if is_space_atom(atom) {
                // The overflowing space is itself the furthest break point
                // and is consumed by the break.
                lines.push(atoms[line_start..index].to_vec());
                line_start = skip_spaces(atoms, index);
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
            // A breaking space that FITS stays at the end of the line
            // (the markdown goldens pin the styled trailing space:
            // `…as one ` carries the paragraph style one cell past
            // "one"). Only a space that would overflow the width is
            // dropped — it never fit.
            if line_end > line_start
                && is_space_atom(&atoms[line_end - 1])
                && atoms[line_start..line_end].iter().map(|atom| atom.width).sum::<usize>() > width
            {
                line_end -= 1;
            }
            // G-467.1 — kinsoku: never open the next line on CJK
            // closing punctuation.
            let line_end = kinsoku_pull_back(atoms, line_start, line_end);
            lines.push(atoms[line_start..line_end].to_vec());
            line_start = skip_spaces(atoms, line_end);
            last_break = None;
            // The overflowing atom opens the next line together with any
            // content already committed past the break point.
            line_width = atoms[line_start..=index].iter().map(|atom| atom.width).sum();
            position = index + 1;
            continue;
        }
        line_width += atom.width;
        if break_after_atom(atom) {
            last_break = Some(index + 1);
        }
        position = index + 1;
    }
    let mut tail_end = atoms.len();
    while tail_end > line_start && is_space_atom(&atoms[tail_end - 1]) {
        tail_end -= 1;
    }
    lines.push(atoms[line_start..tail_end].to_vec());
    lines
}

fn skip_spaces(atoms: &[Atom], mut index: usize) -> usize {
    while atoms.get(index).is_some_and(is_space_atom) {
        index += 1;
    }
    index
}

/// Re-assemble a wrapped line's atoms into runs of contiguous style —
/// the paint-time representation (one run per style, spans split by
/// wrap boundaries yield one run per line fragment).
#[must_use]
pub fn styled_runs(line: &[Atom]) -> Vec<StyledSpan> {
    let mut runs: Vec<StyledSpan> = Vec::new();
    for atom in line {
        match runs.last_mut() {
            Some(run) if run.style == atom.style && run.link == atom.link => {
                run.text.push_str(&atom.text);
            }
            _ => runs.push(StyledSpan {
                text: atom.text.clone(),
                style: atom.style,
                link: atom.link.clone(),
            }),
        }
    }
    runs
}

/// The CSS automatic minimum (min-content) of a plain text leaf (see
/// [`atoms_min_content_width`]).
#[must_use]
pub fn min_content_width(text: &str) -> usize {
    atoms_min_content_width(&flatten(text, PaintStyle::new()))
}

/// The intrinsic single-line (max-content) width of a plain text leaf.
#[must_use]
pub fn max_content_width(text: &str) -> usize {
    display_width(text)
}

/// Longest prefix of `text` fitting `width` display cells
/// (`Locale.takeWidth` — `wrapMode="none"` truncation). Cluster-aware:
/// a prefix never ends inside a grapheme cluster.
#[must_use]
pub fn take_width(text: &str, width: usize) -> String {
    let mut used = 0usize;
    let mut out = String::new();
    for atom in flatten(text, PaintStyle::new()) {
        if used + atom.width > width {
            break;
        }
        used += atom.width;
        out.push_str(&atom.text);
    }
    out
}

/// Wrap `label` into lines of at most `width` display cells (the
/// plain-leaf case of [`wrap_atoms`], joined back to strings).
#[must_use]
pub fn wrap_label(label: &str, width: usize) -> Vec<String> {
    wrap_atoms(&flatten(label, PaintStyle::new()), width)
        .into_iter()
        .map(|line| line.iter().map(|atom| atom.text.as_str()).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLE: PaintStyle = PaintStyle::new();

    fn span(text: &str, style: PaintStyle) -> StyledSpan {
        StyledSpan { text: text.to_owned(), style, link: None }
    }

    fn line_texts(lines: &[Vec<Atom>]) -> Vec<String> {
        lines.iter().map(|line| line.iter().map(|atom| atom.text.as_str()).collect()).collect()
    }

    #[test]
    fn wraps_on_spaces_and_keeps_a_fitting_breaking_space() {
        assert_eq!(wrap_label("aaa bb ccc", 5), vec!["aaa ", "bb ", "ccc"]);
        assert_eq!(wrap_label("aa bb cc", 5), vec!["aa bb", "cc"]);
        // The breaking space stays styled at the line end when it fits
        // (the markdown goldens pin it in the canonical ANSI).
        assert_eq!(wrap_label("one styled", 5), vec!["one ", "style", "d"]);
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

    /// Hyphens are break opportunities (markdown goldens pin the exact
    /// `bold-run-deliberately-` boundary at both 80 and 40 columns).
    #[test]
    fn breaks_at_hyphens_like_the_markdown_golden() {
        assert_eq!(
            wrap_label("bold-run-deliberately-longer-than", 22),
            vec!["bold-run-deliberately-", "longer-than"]
        );
        // The last hyphen that FITS wins, greedy.
        assert_eq!(wrap_label("a-b-c-d", 5), vec!["a-b-", "c-d"]);
        assert_eq!(min_content_width("bold-run-deliberately"), "deliberately".len());
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

    // -- styled-segment stream ------------------------------------------------

    /// Flattening preserves the sidecar clusters: every multi-char
    /// cluster of the corpus is ONE atom whose width the sidecar pins.
    #[test]
    fn flatten_keeps_sidecar_clusters_whole() {
        let family = "👨‍👩‍👧‍👦";
        let atoms = flatten(family, STYLE);
        assert_eq!(atoms.len(), 1, "one ZWJ family = one atom");
        assert_eq!(atoms[0].width, 2);
        // G-452: the atom carries the ref emission shape (head + first
        // ZWJ), not the raw family bytes.
        assert_eq!(atoms[0].text, "\u{1F468}\u{200D}");
        // Skin tones absorb into the preceding emoji.
        let atoms = flatten("👍🏽", STYLE);
        assert_eq!((atoms.len(), atoms[0].width), (1, 2));
        // G-452: the modifier byte is dropped from the emission shape.
        assert_eq!(atoms[0].text, "\u{1F44D}");
        // VS16 + ZWJ (rainbow flag) is one cluster.
        assert_eq!(flatten("🏳️‍🌈", STYLE).len(), 1);
        assert_eq!(flatten("🏳️‍🌈", STYLE)[0].text, "\u{1F3F3}\u{FE0F}\u{200D}");
        // G-452: plain VS16 clusters keep the selector (❤️ stays whole).
        assert_eq!(flatten("❤️", STYLE)[0].text, "\u{2764}\u{FE0F}");
        // Keycap clusters are one atom of width 2.
        let atoms = flatten("1️⃣", STYLE);
        assert_eq!((atoms.len(), atoms[0].width), (1, 2));
        // Regional indicators pair into flags; three pairs = three atoms.
        let atoms = flatten("🇺🇸🇯🇵🇩🇪", STYLE);
        assert_eq!(atoms.len(), 3);
        assert!(atoms.iter().all(|atom| atom.width == 2));
        // NFD combining sequences stay attached to the base.
        assert_eq!(flatten("e\u{301}", STYLE).len(), 1);
        // Devanagari: Mc marks attach, virama forms conjuncts (न्दी = one
        // grapheme of one cell).
        let atoms = flatten("न्दी", STYLE);
        assert_eq!((atoms.len(), atoms[0].width), (1, 1));
        // Atom widths always sum to the string's display width.
        for text in ["thumbs: 👍 plain and 👍🏽 medium skin tone", "हिन्दी में लिखा matras combine"]
        {
            let sum: usize = flatten(text, STYLE).iter().map(|atom| atom.width).sum();
            assert_eq!(sum, display_width(text), "{text:?}");
        }
    }

    /// Gate-9 finding 7: the hard break moves whole clusters — a ZWJ
    /// family never splits (per-char accounting used to cut it between
    /// the base emoji).
    #[test]
    fn hard_breaks_never_split_clusters() {
        let family = "👨‍👩‍👧‍👦";
        // G-452: wrap_label re-assembles from the emission-shaped atoms.
        let shaped = "\u{1F468}\u{200D}";
        let doubled = format!("{family}{family}");
        for width in 1..=4 {
            for line in wrap_label(&doubled, width) {
                // Every line is a concatenation of WHOLE families (both
                // fit at width 4; one per line at 2-3; the width-1
                // overwide-atom quirk mirrors the original seam).
                assert!(
                    line.is_empty() || line == shaped || line == format!("{shaped}{shaped}"),
                    "width {width}: cluster split across lines: {line:?}"
                );
            }
        }
        // Two families at width 2: one per line.
        assert_eq!(wrap_label(&doubled, 2), vec![shaped, shaped]);
    }

    /// A span boundary is NOT a break opportunity: styled spans flow as
    /// one text stream, so a wrap boundary landing mid-span keeps the
    /// span's style on both fragments (the opentui text-children seam).
    #[test]
    fn wrap_breaks_mid_span_and_keeps_style_on_both_fragments() {
        let bold = STYLE.add_modifier(crate::style::Modifier::BOLD);
        let atoms = flatten_spans(&[
            span("plain ", STYLE),
            span("**bold run**", bold),
            span(" tail", STYLE),
        ]);
        let lines = wrap_atoms(&atoms, 12);
        assert_eq!(line_texts(&lines), vec!["plain **bold", "run** tail"]);
        // Line 1: plain prefix + the first half of the bold span.
        assert_eq!(styled_runs(&lines[0]), vec![span("plain ", STYLE), span("**bold", bold)]);
        // Line 2: the second half of the bold span (still bold) + the
        // plain tail — re-assembled per contiguous style.
        assert_eq!(styled_runs(&lines[1]), vec![span("run**", bold), span(" tail", STYLE)]);
    }

    /// Measurement shares the seam with wrapping on styled streams too:
    /// min-content is the longest run between opportunities (span
    /// boundaries irrelevant), max-content the single-line width.
    #[test]
    fn styled_streams_measure_min_and_max_content() {
        let bold = STYLE.add_modifier(crate::style::Modifier::BOLD);
        let atoms =
            flatten_spans(&[span("設定 ", STYLE), span("config", bold), span("の値", STYLE)]);
        assert_eq!(atoms_max_content_width(&atoms), display_width("設定 configの値"));
        // The seam: "configの" (8 cells) is the longest run — identical to
        // the plain leaf of the same text.
        assert_eq!(atoms_min_content_width(&atoms), 8);
        assert_eq!(atoms_min_content_width(&atoms), min_content_width("設定 configの値"));
    }

    /// Cluster safety, systematically: for every width and every
    /// sidecar-pinned sample, wrapping (a) never exceeds the width,
    /// (b) never drops a non-space atom, and (c) never splits a cluster
    /// (the output atoms are exactly the input atoms, minus spaces
    /// consumed at break boundaries).
    #[test]
    fn cluster_safety_property_against_width_sidecars() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/parity/goldens/tui-frames");
        for id in ["width-edge-cjk", "width-edge-emoji", "width-edge-combining"] {
            let sidecar = parity::tui_frame::load_widths_sidecar(&dir, id)
                .unwrap_or_else(|error| panic!("sidecar {id}: {error}"));
            for sample in &sidecar.samples {
                let atoms = flatten(&sample.text, STYLE);
                let sum: usize = atoms.iter().map(|atom| atom.width).sum();
                assert_eq!(sum, display_width(&sample.text), "{id}: {:?}", sample.text);
                for width in 1..=display_width(&sample.text).max(1) {
                    let lines = wrap_atoms(&atoms, width);
                    for line in &lines {
                        let line_width: usize = line.iter().map(|atom| atom.width).sum();
                        // A line fits — except a lone atom that is itself
                        // wider than the wrap width (a wide grapheme at
                        // width 1: the original seam's conservative
                        // overflow, never worse than one cluster).
                        assert!(
                            line_width <= width || (line.len() == 1 && line[0].width == line_width),
                            "{id} {width}: overwide line {line:?}"
                        );
                    }
                    // Merge-walk: output atoms == input atoms, spaces optional.
                    let mut output = lines.iter().flatten();
                    let mut next = output.next();
                    for atom in &atoms {
                        match next {
                            None => assert!(
                                is_space_atom(atom),
                                "{id} {width}: dropped non-space atom {:?}",
                                atom.text
                            ),
                            Some(out) if out.text == atom.text => next = output.next(),
                            Some(_) => assert!(
                                is_space_atom(atom),
                                "{id} {width}: atom {:?} not intact",
                                atom.text
                            ),
                        }
                    }
                    assert!(next.is_none(), "{id} {width}: fabricated atoms");
                }
            }
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

// -- MF#16: the width model lives in `width_model` --------------------------
// The per-class behavior tests (text-emoji narrowing, presentation
// emoji, CJK, standalone marks, `Default_Ignorable`, emoji-regex corpus)
// are `width_model::ref_behavior_tests`; the `width-edge-*` sidecars
// and the anchors below still pin `display_width` end-to-end.

#[cfg(test)]
mod g129_tests {
    use super::*;

    const STYLE: PaintStyle = PaintStyle::new();

    #[test]
    fn heart_and_text_symbols_measure_one_cell() {
        // ❤ / ⚠ / ☀ / ✔ — default-text-presentation emoji: 1 in the
        // ref (string-width.node.ts:9-26) and 1 here (the full-table
        // narrowing; behavior vectors pin these classes).
        assert_eq!(display_width("❤"), 1);
        assert_eq!(display_width("⚠"), 1);
        assert_eq!(display_width("☀"), 1);
        assert_eq!(display_width("✔"), 1);
    }

    #[test]
    fn presentation_and_wide_emoji_stay_two_cells() {
        assert_eq!(display_width("❤️"), 2, "VS16 forces the wide form");
        assert_eq!(display_width("✅"), 2, "`Emoji_Presentation` stays 2");
        assert_eq!(display_width("⌚"), 2);
        assert_eq!(display_width("🙋"), 2);
        assert_eq!(display_width("🇺🇸"), 2, "a flag pair stays 2");
    }

    #[test]
    fn cjk_unaffected_by_the_narrowing() {
        assert_eq!(display_width("宽度"), 4);
        assert_eq!(display_width("ｆｕｌｌ"), 8);
    }

    /// `GB9c` conjuncts per the REF's segmenter (derived tables, MF#16):
    /// Khmer/Myanmar/Tai Tham conjunct; Gurmukhi/Kannada/Sinhala/Tamil
    /// viramas split — each exactly like bun-1.4.2 `Intl.Segmenter`.
    #[test]
    fn conjunct_linkers_follow_the_ref_segmenter() {
        // Devanagari न्द / Khmer ក្ម / Myanmar က္က: ONE cluster, one cell.
        assert_eq!(flatten("न्द", STYLE).len(), 1);
        assert_eq!(display_width("न्द"), 1);
        assert_eq!(display_width("\u{1780}\u{17D2}\u{1798}"), 1, "Khmer coeng conjuncts");
        assert_eq!(flatten("\u{1780}\u{17D2}\u{1798}", STYLE).len(), 1);
        assert_eq!(display_width("\u{1000}\u{1039}\u{1001}"), 1, "Myanmar asat conjuncts");
        // Tamil/Gurmukhi/Kannada/Sinhala viramas do NOT link in the ref.
        assert_eq!(flatten("\u{0B95}\u{0BCD}\u{0B9A}", STYLE).len(), 2, "Tamil splits");
        assert_eq!(flatten("\u{0A15}\u{0A4D}\u{0A15}", STYLE).len(), 2, "Gurmukhi splits");
        assert_eq!(flatten("\u{0C95}\u{0CCD}\u{0C95}", STYLE).len(), 2, "Kannada splits");
        assert_eq!(flatten("\u{0D9A}\u{0DCA}\u{0D9A}", STYLE).len(), 2, "Sinhala splits");
        // A linker + NON-consonant splits (Latin after virama).
        assert_eq!(flatten("\u{0915}\u{094D}a", STYLE).len(), 2);
    }
}
