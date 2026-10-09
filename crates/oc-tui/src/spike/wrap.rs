//! Word-wrap for inline tool-row labels, mirroring the reference opentui
//! text layout for the wrapped-label component class.
//!
//! The reference renderer wraps at *line-break opportunities*, not only at
//! spaces: the exported golden breaks `data.*dir` as `data.` / `*dir` and
//! `Global\.Path` as `Global\.` / `Path` (break after `.`), while `|` and
//! `/` do not create opportunities. Greedy fill: pack characters until one
//! overflows, then break at the last legal opportunity on the line (a
//! breaking space is consumed); if none exists, hard-break at the overflow
//! point. This is the simplified break set the golden corpus exercises — a
//! full port needs the renderer's complete line-break algorithm (see the
//! wave 7.0 report).

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Display width of a string (unicode-width, ambiguous → narrow).
#[must_use]
pub fn display_width(text: &str) -> usize {
    text.width()
}

/// Characters after which a line break is legal (observed in the goldens).
fn is_break_after(ch: char) -> bool {
    ch == ' ' || ch == '.'
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
        let label = r#"Grep "export const OPENCODE_DB|OPENCODE_DB|OPENCODE_DEV|Global\.Path\.data|data =" in packages/opencode/src (115 matches)"#;
        assert_eq!(
            wrap_label(label, 67),
            vec![
                r#"Grep "export const OPENCODE_DB|OPENCODE_DB|OPENCODE_DEV|Global\."#,
                r#"Path\.data|data =" in packages/opencode/src (115 matches)"#,
            ]
        );
    }
}
