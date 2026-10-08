//! Glob matching for file filters, ported from
//! `packages/util/src/glob.ts` (`minimatch` with `dot: true`).
//!
//! Supported pattern subset: `*` (any run within a segment), `**` as a whole
//! segment (any run across segments, and may match zero segments when followed
//! by `/`), `?` (exactly one non-separator character), and `[abc]` / `[a-z]` /
//! `[!a-z]` character classes. Braces and extglobs are not supported; dotfiles
//! always match (the `dot: true` default). Matching is case-sensitive.
//!
//! Directory scanning (`Glob.scan`) needs a filesystem walker and is deferred
//! to the crate that owns that dependency.

/// Matches `filepath` against `pattern`.
#[must_use]
pub fn matches(filepath: &str, pattern: &str) -> bool {
    let input: Vec<char> = filepath.chars().collect();
    let tokens = compile(pattern);
    match_tokens(&input, &tokens)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// Literal character.
    Literal(char),
    /// `?` — one character, never `/`.
    One,
    /// `*` — any run within a segment, never crossing `/`.
    Star,
    /// `**` as a whole segment — any run, may cross `/` and may be empty.
    DoubleStar,
    /// `[...]` character class, never matching `/`.
    Class { negated: bool, items: Vec<ClassItem> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClassItem {
    Char(char),
    Range(char, char),
}

fn compile(pattern: &str) -> Vec<Token> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    // `**` is only a globstar when it forms a complete path segment.
    let mut segment_start = true;
    while index < chars.len() {
        let c = chars[index];
        match c {
            '*' if segment_start
                && chars.get(index + 1) == Some(&'*')
                && !matches!(chars.get(index + 2), Some('*')) =>
            {
                tokens.push(Token::DoubleStar);
                index += 2;
                segment_start = false;
            }
            '*' => {
                tokens.push(Token::Star);
                index += 1;
                segment_start = false;
            }
            '?' => {
                tokens.push(Token::One);
                index += 1;
                segment_start = false;
            }
            '[' => {
                if let Some((token, consumed)) = compile_class(&chars[index..]) {
                    tokens.push(token);
                    index += consumed;
                } else {
                    // Unterminated `[` stays literal.
                    tokens.push(Token::Literal('['));
                    index += 1;
                }
                segment_start = false;
            }
            '/' => {
                tokens.push(Token::Literal('/'));
                index += 1;
                segment_start = true;
            }
            other => {
                tokens.push(Token::Literal(other));
                index += 1;
                segment_start = false;
            }
        }
    }
    tokens
}

/// Parses `[...]` starting at `from_bracket[0] == '['`, returning the class
/// token and the number of consumed characters (brackets included), or `None`
/// when the class never closes.
fn compile_class(from_bracket: &[char]) -> Option<(Token, usize)> {
    let inside = &from_bracket[1..];
    let mut items = Vec::new();
    let mut index = 0;
    let negated = inside.first() == Some(&'!');
    if negated {
        index = 1;
    }
    while index < inside.len() {
        let c = inside[index];
        if c == ']' && !items.is_empty() {
            return Some((
                Token::Class { negated, items },
                index + 2, // ']' plus the opening '['
            ));
        }
        if c == ']' && items.is_empty() {
            // A `]` before any item is a literal.
            items.push(ClassItem::Char(']'));
            index += 1;
            continue;
        }
        let is_range = matches!(inside.get(index + 1), Some('-'))
            && matches!(inside.get(index + 2), Some(hi) if *hi != ']');
        if is_range {
            items.push(ClassItem::Range(c, inside[index + 2]));
            index += 3;
        } else {
            items.push(ClassItem::Char(c));
            index += 1;
        }
    }
    None
}

fn class_matches(items: &[ClassItem], negated: bool, c: char) -> bool {
    if c == '/' {
        return false;
    }
    let hit = items.iter().any(|item| match item {
        ClassItem::Char(expected) => *expected == c,
        ClassItem::Range(lo, hi) => *lo <= c && c <= *hi,
    });
    hit != negated
}

fn match_tokens(input: &[char], tokens: &[Token]) -> bool {
    let Some((head, rest)) = tokens.split_first() else {
        return input.is_empty();
    };
    match head {
        Token::Literal(expected) => {
            !input.is_empty() && input[0] == *expected && match_tokens(&input[1..], rest)
        }
        Token::One => !input.is_empty() && input[0] != '/' && match_tokens(&input[1..], rest),
        Token::Class { negated, items } => {
            !input.is_empty()
                && class_matches(items, *negated, input[0])
                && match_tokens(&input[1..], rest)
        }
        Token::Star => {
            for skip in 0..=input.len() {
                if match_tokens(&input[skip..], rest) {
                    return true;
                }
                if skip < input.len() && input[skip] == '/' {
                    break;
                }
            }
            false
        }
        Token::DoubleStar => {
            // `**/` may swallow the separator along with zero segments.
            if matches!(rest.first(), Some(Token::Literal('/'))) && match_tokens(input, &rest[1..])
            {
                return true;
            }
            for skip in 0..=input.len() {
                if match_tokens(&input[skip..], rest) {
                    return true;
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
    fn literal_paths() {
        assert!(matches("src/a.rs", "src/a.rs"));
        assert!(!matches("src/a.rs", "src/b.rs"));
    }

    #[test]
    fn star_within_segment() {
        assert!(matches("a.rs", "*.rs"));
        assert!(matches("main.rs", "main.*"));
        assert!(!matches("src/a.rs", "*.rs"));
        assert!(!matches("a.rs", "*.ts"));
        assert!(matches("a.rs", "*"));
        assert!(matches("docs", "docs*"));
    }

    #[test]
    fn double_star_crosses_segments() {
        assert!(matches("a/b/c.ts", "**/*.ts"));
        assert!(matches("c.ts", "**/*.ts")); // ** may match zero segments
        assert!(!matches("c.tsx", "**/*.ts"));
        assert!(matches("anything/here", "**"));
        assert!(matches("", "**"));
        assert!(matches("a/b/c", "a/**/c"));
        assert!(matches("a/c", "a/**/c"));
    }

    #[test]
    fn double_star_prefix() {
        assert!(matches("src/deep/nested/a.ts", "src/**/*.ts"));
        assert!(matches("src/a.ts", "src/**/*.ts"));
        assert!(!matches("other/a.ts", "src/**/*.ts"));
    }

    #[test]
    fn star_never_crosses_slashes() {
        assert!(!matches("a/b/c", "a*c"));
        assert!(matches("axc", "a*c"));
    }

    #[test]
    fn embedded_double_star_is_two_stars() {
        // `**` inside a segment behaves like `*` twice (never crosses `/`).
        assert!(!matches("a/b/c", "a**c"));
        assert!(matches("axxc", "a**c"));
    }

    #[test]
    fn question_mark() {
        assert!(matches("cat", "c?t"));
        assert!(!matches("ct", "c?t"));
        assert!(!matches("cart", "c?t"));
        assert!(!matches("c/t", "c?t"));
    }

    #[test]
    fn character_classes() {
        assert!(matches("a.ts", "a.[tj]s"));
        assert!(matches("a.js", "a.[tj]s"));
        assert!(!matches("a.rs", "a.[tj]s"));
        assert!(matches("a1", "a[0-9]"));
        assert!(!matches("aa", "a[0-9]"));
        assert!(matches("ax", "a[!0-9]"));
        assert!(!matches("a1", "a[!0-9]"));
        // Leading `]` is a literal, trailing `-` is a literal.
        assert!(matches("a]x", "a[]]x"));
        assert!(matches("a-x", "a[-]x"));
        assert!(!matches("a]x", "a[[]x"));
    }

    #[test]
    fn unterminated_class_is_literal() {
        assert!(matches("a[b", "a[b"));
    }

    #[test]
    fn dotfiles_match_by_default() {
        assert!(matches(".hidden", "*"));
        assert!(matches(".env", ".*"));
        assert!(matches(".github/workflows/ci.yml", ".github/**"));
    }

    #[test]
    fn case_sensitive() {
        assert!(!matches("A.ts", "a.ts"));
        assert!(matches("A.ts", "A.ts"));
    }

    #[test]
    fn empty_pattern_matches_only_empty() {
        assert!(matches("", ""));
        assert!(!matches("a", ""));
    }
}
