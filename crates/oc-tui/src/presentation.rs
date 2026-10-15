//! `util/presentation.ts` — the post-exit stdout decorations. The
//! session epilogue (#46): the wordmark + resume hint block v2's app
//! writes to stdout AFTER the renderer stops (app.tsx:443-450: the
//! `EpilogueProvider`'s value; session/index.tsx:185-189 sets it while a
//! session route mounts, `onCleanup` clears it leaving one).
//!
//! The bytes are the port's contract: the rig's final-grid capture
//! compares them EXACT (SGR digests included). The string builder
//! was retired with the OC_SUBAGENT flag — the opentui-solid tree
//! (`farewell_solid.rs`) emits the bytes now, consuming these art
//! tables; the byte-shape test pins them through the tree.

/// `logo.left` / `logo.right` (logo.ts:1-4). Pub for the solid
/// farewell's tree (`farewell_solid.rs`) — the ONE art source.
#[cfg(test)]
/// presentation.ts's SGR literals (the byte-shape test's vocabulary).
const RESET: &str = "\x1b[0m";
#[cfg(test)]
const BOLD: &str = "\x1b[1m";
#[cfg(test)]
const DIM: &str = "\x1b[90m";

pub(crate) const LOGO_LEFT: [&str; 4] =
    ["                   ", "█▀▀█ █▀▀█ █▀▀█ █▀▀▄", "█__█ █__█ █^^^ █__█", "▀▀▀▀ █▀▀▀ ▀▀▀▀ ▀~~▀"];
pub(crate) const LOGO_RIGHT: [&str; 4] =
    ["             ▄     ", "█▀▀▀ █▀▀█ █▀▀█ █▀▀█", "█___ █__█ █__█ █^^^", "▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀"];


/// `draw` (presentation.ts:8-17): one wordmark row's glyph cells —
/// `_` paints the background-colored gap, `^` the fg+bg upper-half
/// block, `~` the shadow block, spaces carry the active style.
/// `Locale.truncate(str, len)` (util/locale.ts:50-52): UTF-16 code
/// unit semantics — `length <= len` keeps the string, else the first
/// `len - 1` units + `…`.
#[must_use]
pub fn truncate_utf16(text: &str, len: usize) -> String {
    if text.encode_utf16().count() <= len {
        return text.to_owned();
    }
    let kept: String = text.chars().take(len.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// `sessionEpilogue` (presentation.ts:26-34): the wordmark, a blank
/// line, then the `Session`/`Continue` rows — `weak(label)` dims a
#[cfg(test)]
mod tests {
    use super::*;

    /// The byte shape the rig pins: 8 lines, the `▄` of the wordmark
    /// at row 0 col 35, the padded dim labels, bold values, and the
    /// trailing blank (the app's write appends the final newline).
    /// Promoted with the retirement lane: the bytes are now pinned
    /// through the opentui-solid tree (the only farewell renderer);
    /// the expectations are presentation.ts's own shape.
    #[test]
    fn session_epilogue_bytes_match_presentation_ts() {
        let text = crate::farewell_solid::solid_session_epilogue("Parity Subagent Flow", "ses_abc123");
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), 8, "{lines:?}");
        // Row 0: 2 pad + 19 left + 1 gap + 13 spaces = col 35 `▄`.
        let row0_plain: String = strip_sgr(lines[0]);
        assert_eq!(row0_plain.chars().position(|c| c == '▄'), Some(35), "{row0_plain:?}");
        // Row 1 starts `  █▀▀█ █▀▀█ █▀▀█ █▀▀▄ █▀▀▀ …`.
        assert!(strip_sgr(lines[1]).starts_with("  █▀▀█ █▀▀█ █▀▀█ █▀▀▄ █▀▀▀ █▀▀█ █▀▀█ █▀▀█"));
        // Row 2: `_`→space, `^`→▀.
        assert!(strip_sgr(lines[2]).starts_with("  █  █ █  █ █▀▀▀ █  █ █    █  █ █  █ █▀▀▀"));
        // Row 3: `~`→▀.
        assert!(strip_sgr(lines[3]).starts_with("  ▀▀▀▀ █▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀"));
        assert_eq!(lines[4], "");
        assert_eq!(lines[5], format!("  {DIM}Session   {RESET}{BOLD}Parity Subagent Flow{RESET}"));
        assert_eq!(
            lines[6],
            format!("  {DIM}Continue  {RESET}{BOLD}opencode -s ses_abc123{RESET}")
        );
        assert_eq!(lines[7], "");
        // The exact SGR cells: the `▄` resets to default fg.
        assert!(lines[0].contains("\x1b[0m▄\x1b[0m"));
    }

    /// `Locale.truncate`: ≤50 keeps, 51 truncates to 49 + `…`.
    #[test]
    fn truncate_matches_locale_semantics() {
        assert_eq!(truncate_utf16("abc", 50), "abc");
        let fifty = "x".repeat(50);
        assert_eq!(truncate_utf16(&fifty, 50), fifty);
        let long = "y".repeat(60);
        assert_eq!(truncate_utf16(&long, 50), format!("{}…", "y".repeat(49)));
    }

    fn strip_sgr(line: &str) -> String {
        let mut out = String::new();
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                for pending in chars.by_ref() {
                    if pending.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }
}
