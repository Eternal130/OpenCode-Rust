//! Wildcard pattern matcher for permission rulesets, ported from
//! `packages/core/src/util/wildcard.ts`.
//!
//! Semantics (exact mirror of the regex the TypeScript builds):
//!
//! - Backslashes in both input and pattern become `/` first.
//! - `*` matches any run of characters (including `/` and newlines), `?`
//!   matches exactly one character, everything else is literal (regex
//!   metacharacters never need escaping).
//! - A pattern ending in `" *"` matches both the bare prefix and the prefix
//!   followed by anything: `bash *` matches `bash` and `bash -c ls`. This
//!   comes from the `( .*)?` suffix rewrite in the original.
//! - Matching is case-insensitive on Windows, case-sensitive elsewhere.
//!
//! Permission evaluation (`packages/core/src/permission.ts`) combines this
//! matcher with last-match-wins ruleset scanning; that policy belongs to the
//! consumer, only the matching lives here.

/// Matches `input` against `pattern` with platform case sensitivity
/// (insensitive on Windows, sensitive elsewhere).
#[must_use]
pub fn matches(input: &str, pattern: &str) -> bool {
    matches_with(input, pattern, cfg!(windows))
}

/// Case-insensitive variant of [`matches`].
#[must_use]
pub fn matches_ci(input: &str, pattern: &str) -> bool {
    matches_with(input, pattern, true)
}

/// Matches `input` against `pattern` with explicit case handling.
#[must_use]
pub fn matches_with(input: &str, pattern: &str, case_insensitive: bool) -> bool {
    let input: Vec<char> = input.replace('\\', "/").chars().collect();
    let tokens = compile(pattern);
    match_tokens(&input, &tokens, case_insensitive)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// Literal character.
    Literal(char),
    /// `?` — exactly one character.
    AnyChar,
    /// `*` — any run of characters, possibly empty.
    Star,
    /// Trailing `" *"` — empty or `" "` followed by anything.
    OptionalSpaceStar,
}

fn compile(pattern: &str) -> Vec<Token> {
    let normalized = pattern.replace('\\', "/");
    // "( .*)?" rewrite: a pattern ending in " *" matches the bare prefix too.
    let (source, trailing_optional) = match normalized.strip_suffix(" *") {
        Some(prefix) => (prefix, true),
        None => (normalized.as_str(), false),
    };
    let mut tokens: Vec<Token> = source
        .chars()
        .map(|c| match c {
            '*' => Token::Star,
            '?' => Token::AnyChar,
            other => Token::Literal(other),
        })
        .collect();
    if trailing_optional {
        tokens.push(Token::OptionalSpaceStar);
    }
    tokens
}

fn chars_equal(a: char, b: char, case_insensitive: bool) -> bool {
    if case_insensitive {
        a.to_lowercase().eq(b.to_lowercase())
    } else {
        a == b
    }
}

fn match_tokens(input: &[char], tokens: &[Token], case_insensitive: bool) -> bool {
    let Some((head, rest)) = tokens.split_first() else {
        return input.is_empty();
    };
    match head {
        Token::Literal(expected) => {
            !input.is_empty()
                && chars_equal(input[0], *expected, case_insensitive)
                && match_tokens(&input[1..], rest, case_insensitive)
        }
        Token::AnyChar => !input.is_empty() && match_tokens(&input[1..], rest, case_insensitive),
        Token::Star => {
            for skip in 0..=input.len() {
                if match_tokens(&input[skip..], rest, case_insensitive) {
                    return true;
                }
            }
            false
        }
        Token::OptionalSpaceStar => {
            // Empty alternative.
            if match_tokens(input, rest, case_insensitive) {
                return true;
            }
            // " " + anything alternative.
            if !input.is_empty() && input[0] == ' ' {
                for skip in 1..=input.len() {
                    if match_tokens(&input[skip..], rest, case_insensitive) {
                        return true;
                    }
                }
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals() {
        assert!(matches("read", "read"));
        assert!(matches("bash:run", "bash:run"));
        assert!(!matches("read", "write"));
        assert!(!matches("read", "reading"));
        assert!(!matches("reading", "read"));
    }

    #[test]
    fn star_matches_everything_including_empty() {
        assert!(matches("anything at all", "*"));
        assert!(matches("", "*"));
        assert!(matches("*", "*"));
        assert!(!matches("anything", ""));
    }

    #[test]
    fn empty_pattern_matches_only_empty_input() {
        assert!(matches("", ""));
        assert!(!matches("a", ""));
    }

    #[test]
    fn star_matches_slashes_and_newlines() {
        // `*` compiles to `.*` with the dotall flag in the original.
        assert!(matches("/usr/bin/env", "*env"));
        assert!(matches("a/b/c", "a*c"));
        assert!(matches("a\nb", "a*b"));
    }

    #[test]
    fn multiple_stars() {
        assert!(matches("session:delete:123", "*delete*"));
        assert!(matches("delete", "*delete*"));
        assert!(!matches("remove", "*delete*"));
        assert!(matches("aXbYcZ", "*X*Z"));
        assert!(!matches("aXbY", "*X*Z"));
    }

    #[test]
    fn star_matches_empty_run() {
        assert!(matches("abc", "a*c"));
        assert!(matches("ac", "a*c"));
        assert!(!matches("a", "a*c"));
    }

    #[test]
    fn question_mark_matches_exactly_one_character() {
        assert!(matches("abc", "a?c"));
        assert!(!matches("ac", "a?c"));
        assert!(!matches("abbc", "a?c"));
        assert!(matches("a\nc", "a?c")); // dotall
        assert!(matches("run", "ru?"));
    }

    #[test]
    fn regex_metacharacters_are_literal() {
        assert!(matches("a.b", "a.b"));
        assert!(matches("a+b", "a+b"));
        assert!(!matches("aab", "a+b"));
        assert!(matches("x(y)z", "x(y)z"));
        assert!(matches("a$b", "a$b"));
        assert!(matches("a|b", "a|b"));
        assert!(!matches("axb", "a|b"));
        assert!(matches("a[b", "a[b"));
        assert!(matches("tool:read", "tool:read"));
    }

    #[test]
    fn trailing_space_star_matches_bare_prefix() {
        // The "( .*)?" suffix rule.
        assert!(matches("bash", "bash *"));
        assert!(matches("bash -c ls", "bash *"));
        assert!(matches("bash ", "bash *"));
        assert!(!matches("bashx", "bash *"));
        assert!(!matches("bas", "bash *"));
    }

    #[test]
    fn trailing_space_star_alone() {
        // Pattern " *" compiles to "^( .*)?$": empty or " <anything>".
        assert!(matches("", " *"));
        assert!(matches(" everything", " *"));
        assert!(!matches("everything", " *"));
    }

    #[test]
    fn trailing_space_star_keeps_prefix_literals() {
        assert!(matches("web:fetch", "web:*"));
        assert!(!matches("web", "web:*")); // no space, `*` requires nothing
        assert!(!matches("git:commit", "git: *"));
        assert!(matches("git: ", "git: *"));
        assert!(matches("git: -m x", "git: *"));
    }

    #[test]
    fn double_trailing_star_is_plain_star_run() {
        // "bash **" does not trigger the " *" suffix rewrite (it ends in "**"),
        // so the space stays literal and both stars are plain wildcards.
        assert!(matches("bash anything", "bash **"));
        assert!(matches("bash ", "bash **"));
        assert!(!matches("bash", "bash **"));
    }

    #[test]
    fn backslashes_normalize_to_slashes() {
        assert!(matches("a\\b", "a/b"));
        assert!(matches("a/b", "a\\b"));
        assert!(matches("C:\\dir\\file", "C:*file"));
    }

    #[test]
    fn case_sensitivity() {
        assert!(!matches("Read", "read"));
        assert!(!matches("READ", "read"));
        assert!(matches_ci("Read", "read"));
        assert!(matches_ci("READ", "read"));
        assert!(matches_ci("BASH -C LS", "bash *"));
    }

    #[test]
    fn permission_style_resources() {
        assert!(matches("bash", "bash"));
        assert!(matches("edit", "edit"));
        assert!(matches("/abs/path/file.txt", "*"));
        assert!(matches("project:run:cmd", "*"));
        // Pattern from agent configs.
        assert!(matches("bash:git status", "bash:git *"));
        assert!(!matches("bash:hg status", "bash:git *"));
    }

    #[test]
    fn unicode_input() {
        assert!(matches("héllo wörld", "héllo *"));
        assert!(matches("日本語", "*"));
        assert!(!matches("日本", "日本語"));
    }

    #[test]
    fn compile_produces_expected_tokens() {
        assert_eq!(
            compile("bash *"),
            vec![
                Token::Literal('b'),
                Token::Literal('a'),
                Token::Literal('s'),
                Token::Literal('h'),
                Token::OptionalSpaceStar,
            ]
        );
        assert_eq!(
            compile("a?b*"),
            vec![Token::Literal('a'), Token::AnyChar, Token::Literal('b'), Token::Star,]
        );
    }
}
