//! Identifier conversion + deterministic name allocation.
//!
//! All conversion helpers are pure functions of the input string so that
//! generator output is byte-for-byte deterministic.

use std::collections::HashSet;

const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl", "in",
    "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "union", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// Splits an arbitrary wire identifier into word boundaries.
/// Handles camelCase, `PascalCase`, `UPPER`, `dot.case`, `kebab-case`,
/// `snake_case`, `bracket[case]`.
fn words(input: &str) -> Vec<String> {
    let normalized: Vec<char> =
        input.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' }).collect();
    let normalized: String = normalized.into_iter().collect();
    let mut out = Vec::new();
    for chunk in normalized.split_whitespace() {
        // Split camelCase runs: lower->upper boundary, and upper-run followed by lower.
        let mut current = String::new();
        let chars: Vec<char> = chunk.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            if c.is_ascii_uppercase() {
                let prev_lower = i > 0 && chars[i - 1].is_ascii_lowercase();
                let prev_digit = i > 0 && chars[i - 1].is_ascii_digit();
                let next_lower = chars.get(i + 1).is_some_and(char::is_ascii_lowercase);
                if (prev_lower || prev_digit || next_lower) && !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            current.push(c);
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
}

fn word_lower(w: &str) -> String {
    let mut s: String = w.chars().flat_map(char::to_lowercase).collect();
    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        s.insert(0, 'f');
    }
    s
}

fn word_pascal_nth(w: &str, first: bool) -> String {
    let mut s = String::with_capacity(w.len() + 2);
    for (i, c) in w.chars().enumerate() {
        if i == 0 {
            s.extend(c.to_uppercase());
        } else {
            s.extend(c.to_lowercase());
        }
    }
    // Digit-leading words need an `X` prefix only when they start the ident.
    if first && s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        s.insert(0, 'X');
    }
    s
}

/// `modelID` -> `model_id`, `location[directory]` -> `location_directory`.
pub fn snake_ident(wire: &str) -> String {
    let mut s = String::with_capacity(wire.len() + 4);
    for w in words(wire) {
        if !s.is_empty() {
            s.push('_');
        }
        s.push_str(&word_lower(&w));
    }
    if s.is_empty() {
        s.push_str("field");
    }
    if RUST_KEYWORDS.contains(&s.as_str()) {
        s.push('_');
    }
    s
}

/// `Session.Message.Info` -> `SessionMessageInfo`, `2026-07-28` -> `X20260728`.
pub fn pascal_ident(wire: &str) -> String {
    let parts = words(wire);
    let mut s = String::with_capacity(wire.len() + 2);
    for (i, w) in parts.iter().enumerate() {
        s.push_str(&word_pascal_nth(w, i == 0));
    }
    if s.is_empty() {
        s.push_str("Unnamed");
    }
    s
}

/// Pascal-joins already-snake-clean parts: `["session", "revert_clear"]`.
pub fn pascal_join(parts: &[&str]) -> String {
    let joined = parts.join("_");
    pascal_ident(&joined)
}

/// Deterministic unique-name allocator: appends `2`, `3`, ... on collision.
#[derive(Debug, Default)]
pub struct Names {
    used: HashSet<String>,
}

impl Names {
    pub fn alloc(&mut self, suggest: String) -> String {
        if self.used.insert(suggest.clone()) {
            return suggest;
        }
        let mut n = 2u32;
        loop {
            let candidate = format!("{suggest}{n}");
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
            n += 1;
        }
    }

    pub fn reserve(&mut self, name: &str) {
        self.used.insert(name.to_owned());
    }
}

/// Sanitizes a schema description into a single-line rustdoc fragment.
pub fn doc_line(description: Option<&str>, fallback: &str) -> String {
    let raw = description.unwrap_or(fallback);
    let mut line = String::new();
    let mut last_space = false;
    for c in raw.chars() {
        match c {
            '\n' | '\r' | '\t' => {
                if !last_space {
                    line.push(' ');
                    last_space = true;
                }
            }
            c if c.is_control() => {}
            c => {
                line.push(c);
                last_space = c == ' ';
            }
        }
    }
    let line = line.trim();
    if line.is_empty() {
        return fallback.to_owned();
    }
    if line.len() > 180 {
        // Truncate on a char boundary near the limit.
        let mut end = 180;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &line[..end])
    } else {
        line.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_cases() {
        assert_eq!(snake_ident("modelID"), "model_id");
        assert_eq!(snake_ident("parentID"), "parent_id");
        assert_eq!(snake_ident("location[directory]"), "location_directory");
        assert_eq!(snake_ident("x-opencode-ticket"), "x_opencode_ticket");
        assert_eq!(snake_ident("type"), "type_");
        assert_eq!(snake_ident("expires_in"), "expires_in");
    }

    #[test]
    fn pascal_cases() {
        assert_eq!(pascal_ident("Session.Message.Info"), "SessionMessageInfo");
        assert_eq!(pascal_ident("Form.Fields_1"), "FormFields1");
        assert_eq!(pascal_ident("USD"), "Usd");
        assert_eq!(pascal_ident("2026-07-28"), "X20260728");
        assert_eq!(pascal_ident("-Infinity"), "Infinity");
        assert_eq!(pascal_ident("max_completion_tokens"), "MaxCompletionTokens");
    }
}
