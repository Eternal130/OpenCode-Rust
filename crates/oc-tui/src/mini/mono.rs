//! Mono (ASCII) folding for the scrollback writer — port of
//! `packages/tui/src/mini/mono.ts`. Mini's transcript is written
//! line-by-line into the terminal's native scrollback as plain text;
//! every non-ASCII codepoint folds through these tables so the
//! scrollback stays paste-safe.
//!
//! Ported: `monoText` (`mono.ts:191-193` over the `markdown` table),
//! `monoPrefix` (`mono.ts:222-229` over `prefixes`), `monoToolText`
//! (`mono.ts:231-240`), `monoShortcut` (`mono.ts:242-249`),
//! `monoTruncate` (`mono.ts:251-255`).

#![allow(clippy::match_same_arms)] // the tables mirror mono.ts char-for-char

/// The tool-prefix substitutions (`prefixes`, `mono.ts:17-27`).
fn prefix_replacement(point: char) -> Option<&'static str> {
    Some(match point {
        '\u{2192}' => "->",
        '\u{2190}' => "<-",
        '\u{2731}' => "*",
        '\u{2699}' => "*",
        '\u{2716}' => "!",
        '\u{2717}' => "!",
        '\u{25c8}' => "*",
        '\u{25c9}' => "*",
        '\u{27f3}' => "*",
        _ => return None,
    })
}

/// The full markdown substitution table (`markdown`, `mono.ts:29-46`):
/// the prefixes plus punctuation, quotes, bullets and arrows.
fn markdown_replacement(point: char) -> Option<&'static str> {
    if let Some(prefix) = prefix_replacement(point) {
        return Some(prefix);
    }
    Some(match point {
        '\u{00a0}' => " ",
        '\u{00b7}' => "-",
        '\u{2010}' => "-",
        '\u{2011}' => "-",
        '\u{2012}' => "-",
        '\u{2013}' => "-",
        '\u{2014}' => "--",
        '\u{2018}' => "'",
        '\u{2019}' => "'",
        '\u{201c}' => "\"",
        '\u{201d}' => "\"",
        '\u{2022}' => "*",
        '\u{2026}' => "...",
        '\u{2191}' => "up",
        '\u{2193}' => "down",
        _ => return None,
    })
}

/// `monoText` (`mono.ts:191`): fold every non-tab/newline/non-printable
/// char through the markdown table (`?` when unknown). Control
/// characters other than `\t`/`\n` also fold (`< \x20`).
#[must_use]
pub fn mono_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for char in value.chars() {
        match char {
            '\t' | '\n' => out.push(char),
            '\x20'..='\x7e' => out.push(char),
            other => out.push_str(markdown_replacement(other).unwrap_or("?")),
        }
    }
    out
}

/// `monoPrefix` (`mono.ts:222-229`): replace a leading glyph from the
/// prefix table with its ASCII form, keeping the rest of the value.
#[must_use]
pub fn mono_prefix(value: &str, mono: bool) -> String {
    if !mono {
        return value.to_owned();
    }
    let point = value.chars().next().unwrap_or('\0');
    if let Some(prefix) = prefix_replacement(point) {
        // `value.slice(point > 0xffff ? 2 : 1)` — skip the first
        // UTF-16 unit pair for astral glyphs, one otherwise.
        let rest: String = value.chars().skip(1).collect();
        return format!("{prefix}{rest}");
    }
    value.to_owned()
}

/// The `monoToolText` separator: ` · ` (`U+00B7`).
const TOOL_SEPARATOR: char = '\u{00b7}';

/// `monoToolText` (`mono.ts:231-240`): mono-prefix, then rewrite the
/// LAST ` · ` separator to ` - ` — but only in the tool-caption heads
/// that use it (`... completed`, `patch`, `N questions`).
#[must_use]
pub fn mono_tool_text(value: &str, mono: bool) -> String {
    let result = mono_prefix(value, mono);
    if !mono {
        return result;
    }
    let separator = format!(" {TOOL_SEPARATOR} ");
    let Some(index) = result.rfind(&separator) else { return result };
    let head = &result[..index];
    let completed = head.contains(" completed");
    let patch = head == "patch";
    let questions = head.ends_with(" questions")
        && head[..head.len() - " questions".len()].chars().all(|c| c.is_ascii_digit());
    if !completed && !patch && !questions {
        return result;
    }
    format!("{} - {}", &result[..index], &result[index + separator.len()..])
}

/// `monoShortcut` (`mono.ts:242-249`): arrows to words.
#[must_use]
pub fn mono_shortcut(value: &str, mono: bool) -> String {
    if !mono {
        return value.to_owned();
    }
    value
        .replace('\u{2192}', "right")
        .replace('\u{2190}', "left")
        .replace('\u{2191}', "up")
        .replace('\u{2193}', "down")
}

/// `monoTruncate` (`mono.ts:251-255`): ASCII ellipsis truncation.
/// (The reference slices UTF-16 units; `chars()` is the Rust equivalent
/// for the BMP-heavy captions the writer feeds it.)
#[must_use]
pub fn mono_truncate(value: &str, width: usize, mono: bool) -> String {
    if !mono || value.chars().count() <= width {
        return value.to_owned();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let keep: String = value.chars().take(width - 3).collect();
    format!("{keep}...")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_text_folds_the_markdown_table() {
        // mono.ts:29-46 examples.
        assert_eq!(mono_text("a — b"), "a -- b");
        assert_eq!(mono_text("x · y"), "x - y");
        assert_eq!(mono_text("quote “hi”"), "quote \"hi\"");
        assert_eq!(mono_text("list • …"), "list * ...");
        assert_eq!(mono_text("arrows ↑ ↓"), "arrows up down");
        assert_eq!(mono_text("nbsp\u{00a0}here"), "nbsp here");
        assert_eq!(mono_text("tab\tand\nnewline"), "tab\tand\nnewline");
        assert_eq!(mono_text("emoji \u{1f600}"), "emoji ?", "unknown folds to ?");
    }

    #[test]
    fn mono_prefix_replaces_leading_glyphs() {
        assert_eq!(mono_prefix("→ bash", true), "-> bash");
        assert_eq!(mono_prefix("✗ failed", true), "! failed");
        assert_eq!(mono_prefix("◈ thing", true), "* thing");
        assert_eq!(mono_prefix("plain", true), "plain");
        assert_eq!(mono_prefix("→ kept", false), "→ kept");
    }

    #[test]
    fn mono_tool_text_rewrites_only_completed_separators() {
        assert_eq!(mono_tool_text("Bash · true", false), "Bash · true");
        // Only completion heads rewrite the last separator
        // (`mono.ts:238`): `... completed`, `patch`, `N questions`.
        assert_eq!(
            mono_tool_text("✓ Bash completed · exited 0", true),
            "✓ Bash completed - exited 0"
        );
        assert_eq!(mono_tool_text("3 questions · answered", true), "3 questions - answered");
        assert_eq!(mono_tool_text("patch · file.rs", true), "patch - file.rs");
        // Failures and plain captions keep the separator.
        assert_eq!(mono_tool_text("✗ Bash failed · exited 1", true), "! Bash failed · exited 1");
        assert_eq!(mono_tool_text("Read · src/a.rs", true), "Read · src/a.rs");
    }

    #[test]
    fn mono_shortcut_and_truncate() {
        assert_eq!(mono_shortcut("↑↓←→", true), "updownleftright");
        assert_eq!(mono_shortcut("↑", false), "↑");
        assert_eq!(mono_truncate("abcdefghij", 8, true), "abcde...");
        assert_eq!(mono_truncate("abc", 8, true), "abc");
        assert_eq!(mono_truncate("abcdef", 3, true), "...");
        assert_eq!(mono_truncate("abcdef", 2, true), "..");
        assert_eq!(mono_truncate("abcdef", 10, false), "abcdef");
    }
}
