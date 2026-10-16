//! JSONC edit machinery — a focused port of the `jsonc-parser`
//! behaviors v2's CLI-config service depends on
//! (`packages/cli/src/config/config.ts:92-106`: targeted
//! `modify`/`applyEdits` so toggles rewrite ONE property while the
//! file's comments and formatting survive).
//!
//! Two operations:
//!
//! * [`parse_jsonc`] — parse a JSONC document (line/block comments and
//!   trailing commas tolerated) into a [`serde_json::Value`]. This is
//!   `jsonc-parser`'s `parse(text, { allowTrailingComma: true })`.
//! * [`modify`] — set or delete one property at a JSON path in the raw
//!   text, preserving every byte outside the edited span. This is the
//!   `modify(text, path, value, { formattingOptions: { tabSize: 2,
//!   insertSpaces: true } })` slice v2 uses: inserted values are
//!   pretty-printed at a 2-space indent and new properties land on
//!   their own line.
//!
//! The implementation is a small significant-token scanner (strings
//! with escapes, comments skipped, structure tracked); it never
//! interprets the document semantically beyond the path being edited,
//! so unknown keys, comments, and odd-but-valid JSON layouts
//! round-trip untouched.

use serde_json::Value;

/// One significant token in a JSONC document.
#[derive(Debug, Clone, PartialEq)]
struct Token {
    kind: TokenKind,
    /// Byte offset of the token's first character.
    start: usize,
    /// Byte offset one past the token's last character.
    end: usize,
    /// The unescaped text of a string token (names/keys only).
    text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    Colon,
    Comma,
    String,
    /// A bare literal: number / `true` / `false` / `null`.
    Literal,
}

struct Scanner<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, bytes: text.as_bytes(), pos: 0 }
    }

    /// Skips whitespace and comments (line `//…` and block `/*…*/`).
    fn skip_insignificant(&mut self) -> Result<(), String> {
        loop {
            self.skip_whitespace();
            match self.bytes.get(self.pos) {
                Some(b'/') => match self.bytes.get(self.pos + 1) {
                    Some(b'/') => {
                        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
                            self.pos += 1;
                        }
                    }
                    Some(b'*') => {
                        self.pos += 2;
                        loop {
                            if self.pos + 1 >= self.bytes.len() {
                                return Err("unterminated block comment".to_owned());
                            }
                            if self.bytes[self.pos] == b'*' && self.bytes[self.pos + 1] == b'/' {
                                self.pos += 2;
                                break;
                            }
                            self.pos += 1;
                        }
                    }
                    _ => return Ok(()),
                },
                _ => return Ok(()),
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    /// Scans the next significant token, or `None` at end of input.
    fn next(&mut self) -> Result<Option<Token>, String> {
        self.skip_insignificant()?;
        let Some(&first) = self.bytes.get(self.pos) else { return Ok(None) };
        let kind = match first {
            b'{' => TokenKind::OpenBrace,
            b'}' => TokenKind::CloseBrace,
            b'[' => TokenKind::OpenBracket,
            b']' => TokenKind::CloseBracket,
            b':' => TokenKind::Colon,
            b',' => TokenKind::Comma,
            b'"' => TokenKind::String,
            b'-' | b'0'..=b'9' | b't' | b'f' | b'n' => TokenKind::Literal,
            other => return Err(format!("unexpected byte {:?} at {}", other as char, self.pos)),
        };
        let start = self.pos;
        let text = if kind == TokenKind::String {
            self.scan_string(start)?
        } else {
            self.pos += 1;
            None
        };
        if kind == TokenKind::Literal {
            while self.pos < self.bytes.len()
                && matches!(self.bytes[self.pos], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9' | b'a'..=b'z')
            {
                self.pos += 1;
            }
        }
        Ok(Some(Token { kind, start, end: self.pos, text }))
    }

    /// Scans a string starting at `start` (the opening quote), returning
    /// its unescaped content.
    fn scan_string(&mut self, start: usize) -> Result<Option<String>, String> {
        debug_assert_eq!(self.bytes[self.pos], b'"');
        self.pos += 1;
        let mut out = String::new();
        loop {
            let Some(&byte) = self.bytes.get(self.pos) else {
                return Err("unterminated string".to_owned());
            };
            match byte {
                b'"' => {
                    self.pos += 1;
                    let _ = start;
                    return Ok(Some(out));
                }
                b'\\' => {
                    let Some(&escape) = self.bytes.get(self.pos + 1) else {
                        return Err("unterminated escape".to_owned());
                    };
                    self.pos += 2;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self
                                .text
                                .get(self.pos..self.pos + 4)
                                .ok_or_else(|| "truncated \\u escape".to_owned())?;
                            let code = u32::from_str_radix(hex, 16)
                                .map_err(|_| "invalid \\u escape".to_owned())?;
                            self.pos += 4;
                            // Surrogate pairs: combine when followed by a
                            // low surrogate, else emit U+FFFD like JSON.
                            let ch = if (0xD800..0xDC00).contains(&code) {
                                let combined = char::REPLACEMENT_CHARACTER;
                                if self.bytes.get(self.pos) == Some(&b'\\')
                                    && self.bytes.get(self.pos + 1) == Some(&b'u')
                                {
                                    if let Some(hex2) = self.text.get(self.pos + 2..self.pos + 6) {
                                        if let Ok(low) = u32::from_str_radix(hex2, 16) {
                                            if (0xDC00..0xE000).contains(&low) {
                                                let joined = 0x10000
                                                    + ((code - 0xD800) << 10)
                                                    + (low - 0xDC00);
                                                if let Some(joined) = char::from_u32(joined) {
                                                    self.pos += 6;
                                                    joined
                                                } else {
                                                    combined
                                                }
                                            } else {
                                                combined
                                            }
                                        } else {
                                            combined
                                        }
                                    } else {
                                        combined
                                    }
                                } else {
                                    combined
                                }
                            } else {
                                char::from_u32(code).unwrap_or('\u{FFFD}')
                            };
                            out.push(ch);
                        }
                        _ => return Err("invalid escape".to_owned()),
                    }
                }
                _ => {
                    // Copy one UTF-8 sequence verbatim.
                    let len = utf8_len(byte);
                    let end = (self.pos + len).min(self.bytes.len());
                    out.push_str(self.text.get(self.pos..end).unwrap_or("\u{FFFD}"));
                    self.pos = end;
                }
            }
        }
    }
}

fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

/// Parse a JSONC document (comments + trailing commas allowed). A JSON
/// value of any kind is accepted (the config layer treats non-objects
/// as absent, mirroring v2's `parseRecord`).
///
/// # Errors
///
/// Returns a message on syntax errors (the caller logs and skips the
/// file — a bad config never bricks the app).
pub fn parse_jsonc(text: &str) -> Result<Value, String> {
    let mut stripped = strip_comments(text)?;
    strip_trailing_commas(&mut stripped);
    serde_json::from_str(&stripped).map_err(|error| error.to_string())
}

/// Replaces every comment's bytes with spaces (same length, so offsets
/// in the stripped text line up with the source for callers that need
/// both).
fn strip_comments(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if byte == b'"' {
            // Skip the string, honoring escapes.
            let mut scanner_at = Scanner { text, bytes, pos };
            scanner_at.scan_string(pos)?;
            pos = scanner_at.pos;
            continue;
        }
        if byte == b'/' && bytes.get(pos + 1) == Some(&b'/') {
            while pos < bytes.len() && bytes[pos] != b'\n' {
                out[pos] = b' ';
                pos += 1;
            }
            continue;
        }
        if byte == b'/' && bytes.get(pos + 1) == Some(&b'*') {
            let start = pos;
            pos += 2;
            while pos + 1 < bytes.len() && !(bytes[pos] == b'*' && bytes[pos + 1] == b'/') {
                pos += 1;
            }
            if pos + 1 >= bytes.len() {
                return Err("unterminated block comment".to_owned());
            }
            pos += 2;
            for slot in &mut out[start..pos] {
                if *slot != b'\n' {
                    *slot = b' ';
                }
            }
            continue;
        }
        pos += 1;
    }
    String::from_utf8(out).map_err(|_| "invalid utf-8".to_owned())
}

/// Removes trailing commas (a comma whose next significant token is
/// `}` or `]`) in place — `jsonc-parser`'s `allowTrailingComma`.
fn strip_trailing_commas(text: &mut String) {
    let mut out = text.as_bytes().to_vec();
    let mut scanner = Scanner::new(text);
    let mut previous_end = 0usize;
    let mut gap_comma = None; // (gap start, gap end) of the last comma seen
    let mut tokens = Vec::new();
    while let Ok(Some(token)) = scanner.next() {
        if token.kind == TokenKind::Comma {
            gap_comma = Some((previous_end, token.end));
        } else {
            if matches!(token.kind, TokenKind::CloseBrace | TokenKind::CloseBracket) {
                if let Some((start, end)) = gap_comma {
                    for slot in &mut out[start..end] {
                        if *slot == b',' {
                            *slot = b' ';
                            break;
                        }
                    }
                }
            }
            gap_comma = None;
        }
        previous_end = token.end;
        tokens.push(token);
    }
    let _ = tokens.len();
    if let Ok(stripped) = String::from_utf8(out) {
        *text = stripped;
    }
}

/// Set (`Some`) or delete (`None`) the property at `path` in the raw
/// JSONC `text`, returning the edited text. Every byte outside the
/// edited span — comments, key order, unrelated properties, custom
/// formatting — is preserved (v2's targeted `modify` contract).
///
/// Deletion of a missing property is a no-op. Setting a property
/// inside a missing or non-object parent creates the parent objects
/// (`modify`'s behavior with `formattingOptions`).
///
/// # Errors
///
/// Returns a message on syntax errors or when the path cannot be
/// applied (e.g. the root is not an object).
pub fn modify(text: &str, path: &[&str], value: Option<&Value>) -> Result<String, String> {
    if path.is_empty() {
        return Err("empty path".to_owned());
    }
    let mut current = text_from(text);
    // Materialize every intermediate object along the path.
    for depth in 1..path.len() {
        let parent = &path[..depth - 1];
        let segment = path[depth - 1];
        current = ensure_object_at(&current, parent, segment)?;
    }
    let parent = &path[..path.len() - 1];
    let key = path[path.len() - 1];
    match value {
        Some(value) => set_property(&current, parent, key, value),
        None => delete_property(&current, parent, key),
    }
}

fn text_from(text: &str) -> String {
    if has_tokens(text) {
        text.to_owned()
    } else {
        "{\n}".to_owned()
    }
}

fn has_tokens(text: &str) -> bool {
    !text.bytes().all(|b| b.is_ascii_whitespace())
}

/// The span of one property: `key` token through the end of its value.
struct PropertySpan {
    key_start: usize,
    value_start: usize,
    /// The byte range of the whole property's VALUE (for replacement).
    value_end: usize,
    /// Whether a comma token sits immediately before this property
    /// (after the previous property or `{`).
    comma_before: Option<(usize, usize)>,
    /// Whether a comma token sits immediately after this property's
    /// value.
    comma_after: Option<(usize, usize)>,
}

/// Find `key`'s property inside the object addressed by `path`
/// (empty path = the root object). `None` when the object exists but
/// has no such property; `Err` when the path does not address an
/// object or the document is malformed.
fn find_property(text: &str, path: &[String], key: &str) -> Result<Option<PropertySpan>, String> {
    let object = find_object(text, path)?;
    let Some(object) = object else { return Ok(None) };
    let mut scanner = Scanner::new(text);
    seek(&mut scanner, object.open_after)?;
    // Walk properties: String, Colon, <value>, optional Comma.
    let mut comma_before = None;
    loop {
        let Some(token) = scanner.next()? else { return Err("unterminated object".to_owned()) };
        match token.kind {
            TokenKind::CloseBrace => return Ok(None),
            TokenKind::Comma => {
                comma_before = Some((token.start, token.end));
                continue;
            }
            TokenKind::String => {}
            _ => return Err("expected property name".to_owned()),
        }
        let name = token.text.clone().unwrap_or_default();
        let key_start = token.start;
        let colon = scanner.next()?.ok_or("expected :")?;
        if colon.kind != TokenKind::Colon {
            return Err("expected : after property name".to_owned());
        }
        let (value_start, value_end) = skip_value_span(&mut scanner)?;
        if name == key {
            // Peek for a following comma.
            let mut peek = scanner.clone_position();
            let comma_after =
                peek.next()?.filter(|t| t.kind == TokenKind::Comma).map(|t| (t.start, t.end));
            return Ok(Some(PropertySpan {
                key_start,
                value_start,
                value_end,
                comma_before,
                comma_after,
            }));
        }
        // Advance past an optional separating comma (it becomes the
        // next property's leading comma).
        let mut peek = scanner.clone_position();
        if let Some(token) = peek.next()? {
            if token.kind == TokenKind::Comma {
                comma_before = Some((token.start, token.end));
                scanner = peek;
                continue;
            }
        }
        comma_before = None;
    }
}

/// The character range of an object `{…}` addressed by `path`.
struct ObjectSpan {
    /// Offset just past the `{`.
    open_after: usize,
    /// Offset of the matching `}`.
    close: usize,
    /// Whether the object contains at least one property.
    has_properties: bool,
}

fn find_object(text: &str, path: &[String]) -> Result<Option<ObjectSpan>, String> {
    let mut scanner = Scanner::new(text);
    let Some(root) = scanner.next()? else { return Ok(None) };
    if root.kind != TokenKind::OpenBrace {
        return Ok(None);
    }
    let mut open_after = root.end;
    for segment in path {
        // Find `segment` inside the current object, then require its
        // value to be an object.
        let mut found: Option<usize> = None; // offset of the value's `{`
        loop {
            let Some(token) = scanner.next()? else { return Err("unterminated object".to_owned()) };
            match token.kind {
                TokenKind::CloseBrace => break,
                TokenKind::Comma => {}
                TokenKind::String => {
                    let name = token.text.clone().unwrap_or_default();
                    let colon = scanner.next()?.ok_or("expected :")?;
                    if colon.kind != TokenKind::Colon {
                        return Err("expected :".to_owned());
                    }
                    let value_token = scanner.next()?.ok_or("expected value")?;
                    if name == *segment && value_token.kind == TokenKind::OpenBrace {
                        found = Some(value_token.start);
                    }
                    rewind_value(&mut scanner, &value_token)?;
                }
                _ => return Err("expected property name".to_owned()),
            }
        }
        let Some(offset) = found else { return Ok(None) };
        // Seek the scanner INSIDE the found object (just past its `{`)
        // so the next iteration (and the final probe) walk properties.
        let mut seek_scanner = Scanner::new(text);
        seek_scanner.pos = offset + 1;
        seek_scanner.skip_insignificant()?;
        scanner = seek_scanner;
        open_after = offset + 1;
    }
    // The scanner now sits just inside the final object.
    let mut has_properties = false;
    let mut probe = scanner.clone_position();
    if let Some(token) = probe.next()? {
        has_properties = token.kind != TokenKind::CloseBrace;
    }
    let close = matching_close(text, open_after)?;
    Ok(Some(ObjectSpan { open_after, close, has_properties }))
}

/// Move the scanner so the NEXT token starts at or after `offset`.
fn seek(scanner: &mut Scanner, offset: usize) -> Result<(), String> {
    scanner.pos = offset;
    scanner.skip_insignificant()
}

/// Advance the scanner past one complete value, returning the value's
/// `(start, end)` span (past the closing bracket for composites).
fn skip_value_span(scanner: &mut Scanner) -> Result<(usize, usize), String> {
    let token = scanner.next()?.ok_or("expected value")?;
    let start = token.start;
    rewind_value(scanner, &token)?;
    Ok((start, scanner.pos))
}

/// Advance the scanner past one complete value, returning the value's
/// end offset.
fn skip_value(scanner: &mut Scanner) -> Result<usize, String> {
    Ok(skip_value_span(scanner)?.1)
}

/// After `token` has been consumed, advance past its remainder (for
/// compound values the scanner is at the element start, not past it).
fn rewind_value(scanner: &mut Scanner, token: &Token) -> Result<(), String> {
    match token.kind {
        TokenKind::OpenBrace | TokenKind::OpenBracket => {
            let mut depth = 1usize;
            while depth > 0 {
                let next = scanner.next()?.ok_or("unterminated composite value")?;
                match next.kind {
                    TokenKind::OpenBrace | TokenKind::OpenBracket => depth += 1,
                    TokenKind::CloseBrace | TokenKind::CloseBracket => depth -= 1,
                    _ => {}
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Offset of the bracket matching the `}`/`]` for the composite value
/// that opens at/before `inside` (an offset just past the opening
/// bracket).
fn matching_close(text: &str, inside: usize) -> Result<usize, String> {
    let mut scanner = Scanner::new(text);
    seek(&mut scanner, inside.checked_sub(1).ok_or("offset underflow")?)?;
    let open = scanner.next()?.ok_or("expected composite value")?;
    let (open_kind, close_kind) = match open.kind {
        TokenKind::OpenBrace => (TokenKind::OpenBrace, TokenKind::CloseBrace),
        TokenKind::OpenBracket => (TokenKind::OpenBracket, TokenKind::CloseBracket),
        _ => return Err("not a composite value".to_owned()),
    };
    let mut depth = 1usize;
    while depth > 0 {
        let token = scanner.next()?.ok_or("unterminated composite value")?;
        if token.kind == open_kind {
            depth += 1;
        } else if token.kind == close_kind {
            depth -= 1;
            if depth == 0 {
                return Ok(token.start);
            }
        }
    }
    Err("unterminated composite value".to_owned())
}

/// Serialize an inserted value the way v2's `modify` does with
/// `tabSize: 2, insertSpaces: true`: scalars compact, composites
/// pretty-printed at a 2-space indent.
fn serialize_inserted(value: &Value) -> String {
    match value {
        Value::Object(_) | Value::Array(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_owned())
        }
        _ => serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned()),
    }
}

/// The indentation for a new property: two spaces deeper than the
/// whitespace prefix of the line holding the object's closing bracket
/// (matching the reference's `tabSize: 2, insertSpaces: true`). A
/// closing bracket that shares its line with content (single-line
/// object) falls back to the root indent — the insertion converts the
/// object to one-property-per-line layout, as the reference's modify
/// does.
fn insertion_indent(text: &str, close: usize) -> String {
    let line_start = text[..close].rfind('\n').map_or(0, |index| index + 1);
    let before_close = &text[line_start..close];
    if before_close.chars().all(char::is_whitespace) {
        format!("{before_close}  ")
    } else {
        "  ".to_owned()
    }
}

/// Ensure the object at `path` has a(n object) property `segment`.
fn ensure_object_at(text: &str, path: &[&str], segment: &str) -> Result<String, String> {
    let owned: Vec<String> = path.iter().map(|s| (*s).to_owned()).collect();
    let mut current = text.to_owned();
    // How deep do objects already exist along the path?
    let mut depth = 0usize;
    while depth <= owned.len() && find_object(&current, &owned[..depth])?.is_some() {
        depth += 1;
    }
    // `depth` now points one past the deepest existing prefix; create
    // every missing segment below it as an object.
    while depth < owned.len() {
        let parent: Vec<&str> = owned[..depth].iter().map(String::as_str).collect();
        current = insert_property(&current, &parent, &owned[depth], &serde_json::json!({}))?;
        depth += 1;
    }
    // The object at `path` exists now; does the property exist?
    if let Some(property) = find_property(&current, &owned, segment)? {
        // Exists: its value must be an object — replace it when not.
        let value_text = &current[property.value_start..property.value_end];
        if parse_jsonc(value_text).is_ok_and(|v| v.is_object()) {
            return Ok(current);
        }
        return Ok(splice(&current, property.value_start, property.value_end, "{}"));
    }
    insert_property(&current, path, segment, &serde_json::json!({}))
}

/// Insert `key: value` into the object at `path` (which must exist).
fn insert_property(text: &str, path: &[&str], key: &str, value: &Value) -> Result<String, String> {
    let owned: Vec<String> = path.iter().map(|s| (*s).to_owned()).collect();
    let Some(object) = find_object(text, &owned)? else {
        return Err(format!("object at {} does not exist", path.join("/")));
    };
    let indent = insertion_indent(text, object.close);
    let serialized = serialize_inserted(value);
    let key_json = serde_json::to_string(key).unwrap_or_else(|_| format!("{key:?}"));
    if object.has_properties {
        // Append after the last property, before the close bracket:
        // `,\n<indent>"key": value`.
        let last_end = last_property_end(text, object.open_after, object.close)?;
        let insertion = format!(",\n{indent}{key_json}: {serialized}");
        Ok(splice(text, last_end, last_end, &insertion))
    } else {
        // `{}` → `{\n<indent>"key": value\n<base>}`.
        let base = indent[..indent.len() - 2].to_owned();
        let insertion = format!("\n{indent}{key_json}: {serialized}\n{base}");
        Ok(splice(text, object.close, object.close, &insertion))
    }
}

/// End offset of the last property's value inside the object span.
fn last_property_end(text: &str, open_after: usize, close: usize) -> Result<usize, String> {
    let mut scanner = Scanner::new(text);
    seek(&mut scanner, open_after)?;
    let mut last_end = None;
    while let Some(token) = scanner.next()? {
        match token.kind {
            TokenKind::CloseBrace => break,
            TokenKind::Comma => {}
            TokenKind::String => {
                let colon = scanner.next()?.ok_or("expected :")?;
                if colon.kind != TokenKind::Colon {
                    return Err("expected :".to_owned());
                }
                let end = skip_value(&mut scanner)?;
                last_end = Some(end);
            }
            _ => return Err("expected property name".to_owned()),
        }
    }
    last_end.ok_or_else(|| format!("object at {open_after}..{close} has no properties"))
}

/// Set `key`'s value inside the object at `path`.
fn set_property(text: &str, path: &[&str], key: &str, value: &Value) -> Result<String, String> {
    let owned: Vec<String> = path.iter().map(|s| (*s).to_owned()).collect();
    if let Some(property) = find_property(text, &owned, key)? {
        let serialized = serialize_inserted(value);
        Ok(splice(text, property.value_start, property.value_end, &serialized))
    } else {
        insert_property(text, path, key, value)
    }
}

/// Delete `key`'s property inside the object at `path` (no-op when
/// absent).
fn delete_property(text: &str, path: &[&str], key: &str) -> Result<String, String> {
    let owned: Vec<String> = path.iter().map(|s| (*s).to_owned()).collect();
    let Some(property) = find_property(text, &owned, key)? else { return Ok(text.to_owned()) };
    // Prefer removing the leading comma (previous property keeps its
    // trailing comma position); fall back to the trailing one.
    if let Some((start, _)) = property.comma_before {
        return Ok(splice(text, start, property.value_end, ""));
    }
    if let Some((_, end)) = property.comma_after {
        return Ok(splice(text, property.key_start, end, ""));
    }
    Ok(splice(text, property.key_start, property.value_end, ""))
}

fn splice(text: &str, start: usize, end: usize, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(&text[..start]);
    out.push_str(replacement);
    out.push_str(&text[end..]);
    out
}

impl<'a> Scanner<'a> {
    /// A scanner positioned where this one is (used for one-token
    /// lookahead).
    fn clone_position(&self) -> Scanner<'a> {
        Scanner { text: self.text, bytes: self.bytes, pos: self.pos }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_jsonc_with_comments_and_trailing_commas() {
        let text = r#"{
          // theme settings
          "theme": { "name": "catppuccin", /* inline */ },
          "mouse": false,
        }"#;
        let value = parse_jsonc(text).expect("parses");
        assert_eq!(value["theme"]["name"], "catppuccin");
        assert_eq!(value["mouse"], false);
    }

    #[test]
    fn parse_rejects_broken_documents() {
        assert!(parse_jsonc("{ broken").is_err());
        assert!(parse_jsonc("\"unterminated").is_err());
    }

    #[test]
    fn modify_sets_a_nested_property_preserving_comments() {
        let text = r#"{
          // keep me
          "theme": {
            "name": "catppuccin" // trailing comment
          },
          "mouse": true
        }"#;
        let updated = modify(text, &["theme", "mode"], Some(&json!("dark"))).expect("modify");
        assert!(updated.contains("// keep me"), "comments survive: {updated}");
        assert!(updated.contains("\"mode\": \"dark\""));
        assert!(updated.contains("\"name\": \"catppuccin\""));
        // Round-trip: the edited document still parses with the value set.
        let value = parse_jsonc(&updated).expect("still parses");
        assert_eq!(value.pointer("/theme/mode"), Some(&json!("dark")));
        assert_eq!(value.pointer("/theme/name"), Some(&json!("catppuccin")));
    }

    #[test]
    fn modify_replaces_an_existing_scalar() {
        let text = r#"{
  "mouse": true,
  "animations": false
}"#;
        let updated = modify(text, &["mouse"], Some(&json!(false))).expect("modify");
        let value = parse_jsonc(&updated).expect("parses");
        assert_eq!(value["mouse"], false);
        assert_eq!(value["animations"], false, "sibling untouched");
        assert!(updated.contains("\"animations\": false"));
    }

    #[test]
    fn modify_creates_missing_parent_objects() {
        let text = "{\n  \"mouse\": true\n}\n";
        let updated = modify(text, &["mini", "replay"], Some(&json!(false))).expect("modify");
        let value = parse_jsonc(&updated).expect("parses");
        assert_eq!(value.pointer("/mini/replay"), Some(&json!(false)));
        assert_eq!(value.pointer("/mouse"), Some(&json!(true)));
    }

    #[test]
    fn modify_replaces_a_non_object_parent() {
        let text = "{\n  \"theme\": \"oops\"\n}\n";
        let updated = modify(text, &["theme", "mode"], Some(&json!("light"))).expect("modify");
        let value = parse_jsonc(&updated).expect("parses");
        assert_eq!(value.pointer("/theme/mode"), Some(&json!("light")));
    }

    #[test]
    fn modify_deletes_a_property_with_its_comma() {
        let text = "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}\n";
        let updated = modify(text, &["b"], None).expect("delete");
        let value = parse_jsonc(&updated).expect("parses");
        assert!(value.get("b").is_none());
        assert_eq!(value["a"], 1);
        assert_eq!(value["c"], 3);
        // Deleting the first and last properties also stays valid JSON.
        let updated = modify(&updated, &["a"], None).expect("delete a");
        let updated = modify(&updated, &["c"], None).expect("delete c");
        let value = parse_jsonc(&updated).expect("parses");
        assert_eq!(value.as_object().map(|o| o.len()), Some(0));
    }

    #[test]
    fn modify_on_an_empty_document_creates_the_root() {
        let updated = modify("", &["animations"], Some(&json!(false))).expect("modify");
        let value = parse_jsonc(&updated).expect("parses");
        assert_eq!(value["animations"], false);
    }

    #[test]
    fn modify_inserts_pretty_composites() {
        let text = "{}";
        let updated = modify(text, &["attention"], Some(&json!({"volume": 0.8}))).expect("modify");
        assert!(
            updated.contains("\"volume\": 0.8"),
            "composite values insert pretty-printed: {updated}"
        );
    }

    #[test]
    fn modify_into_an_empty_nested_object() {
        let text = "{\n  \"attention\": {}\n}\n";
        let updated = modify(text, &["attention", "sound"], Some(&json!(true))).expect("modify");
        let value = parse_jsonc(&updated).expect("parses");
        assert_eq!(value.pointer("/attention/sound"), Some(&json!(true)));
        assert!(updated.contains('\n'), "property lands on its own line: {updated}");
    }

    #[test]
    fn modify_survives_a_round_trip_of_many_edits() {
        let mut text = "{\n  \"$schema\": \"https://opencode.ai/v2/cli.json\"\n}".to_owned();
        let edits: Vec<(Vec<&str>, Value)> = vec![
            (vec!["mouse"], json!(false)),
            (vec!["theme", "mode"], json!("dark")),
            (vec!["mini", "replay"], json!(false)),
            (vec!["mini", "replay_limit"], json!(50)),
            (vec!["attention", "volume"], json!(0.9)),
            (vec!["experimental", "flags.foo"], json!(true)),
        ];
        for (path, value) in edits {
            text = modify(&text, &path, Some(&value)).expect("modify");
        }
        let value = parse_jsonc(&text).expect("parses");
        assert_eq!(value.pointer("/mouse"), Some(&json!(false)));
        assert_eq!(value.pointer("/theme/mode"), Some(&json!("dark")));
        assert_eq!(value.pointer("/mini/replay"), Some(&json!(false)));
        assert_eq!(value.pointer("/mini/replay_limit"), Some(&json!(50)));
        assert_eq!(value.pointer("/attention/volume"), Some(&json!(0.9)));
        assert_eq!(value.pointer("/experimental/flags.foo"), Some(&json!(true)));
        assert!(text.contains("\"$schema\": \"https://opencode.ai/v2/cli.json\""));
    }
}
