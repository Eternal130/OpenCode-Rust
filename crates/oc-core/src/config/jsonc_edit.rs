//! JSONC surgical editor — D-01 comment-preserving config write-back.
//!
//! Every v2 writer that mutates an existing config file does so through
//! jsonc-parser's `modify`/`applyEdits` with `formattingOptions: { tabSize:
//! 2, insertSpaces: true }`, rewriting only the touched ranges and leaving
//! comments/formatting intact:
//!
//! - `packages/core/src/config.ts` `Config.update` — sets/removes
//!   `["shell"]` (config.ts:329-349);
//! - `packages/cli/src/commands/handlers/mcp/add.ts` `write` — sets
//!   `["mcp", "servers", <name>]` (add.ts:59-68);
//! - `packages/cli/src/commands/handlers/plugin/add.ts`
//!   `writePluginConfig` and `plugin/remove.ts` `removePluginConfig` —
//!   set `["plugins"]` to the whole new array.
//!
//! This module ports jsonc-parser@3.3.1's pipeline for those call shapes:
//!
//! - scanner (`impl/scanner.js`): byte-offset tokens over JSONC (comments
//!   and line breaks are tokens; strings decode with escapes);
//! - tolerant tree parser (`impl/parser.js` `parseTree`): node spans
//!   (object/array/property/literal) with offsets and lengths; parse
//!   errors never abort, matching `setProperty`'s error-blindness;
//! - splice planner (`impl/edit.js` `setProperty`): ONE edit that replaces
//!   a value range, deletes a property span (with comma cleanup), or
//!   inserts a new property (`"key": <compact json>`, comma-prefixed
//!   after the last property / bare just inside an empty object), with
//!   missing intermediate objects wrapped around the value;
//! - range formatter (`impl/format.js` `format`): the touched lines are
//!   re-indented (2 spaces, `": "` after colons, newline after `{`/`[`/`,`
//!   and before `}`/`]`); comments participate as tokens and are kept
//!   (padded with a single space), never deleted by this pass.
//!
//! [`set_path`] returns the final text; every byte outside the splice and
//! its re-formatted line range survives verbatim (verified against a
//! golden corpus generated with the real jsonc-parser — see
//! `jsonc_edit_fixtures.json`).
//!
//! Registered divergences from v2 (byte-level):
//! - numeric path segments (array indices) are unsupported — no v2 D-01
//!   writer produces them (arrays are always replaced wholesale, so the
//!   array branch of `setProperty` is dead code for this port);
//! - multi-key inserted values serialize with keys sorted: the workspace
//!   drops the `serde_json` `preserve_order` feature (upstream v2
//!   `JSON.stringify` keeps insertion order). Affects only inserted
//!   objects with 2+ keys;
//! - a formatter indent level that would go negative (reachable only from
//!   malformed documents) saturates at zero instead of raising like JS
//!   `String.repeat` — the affected edit is range-dropped regardless.

use serde_json::Value;

/// Why a surgical edit could not be produced. Mirrors the errors
/// jsonc-parser's `modify` throws for the same shapes.
#[derive(Debug, Eq, PartialEq)]
pub enum ModifyError {
    /// `Can not delete in empty document` — removal against text with no
    /// root value (empty or comments-only).
    DeleteInEmptyDocument,
    /// `Can not add property to parent of type <kind>` — the target's
    /// parent exists but is not a JSON object.
    AddPropertyTo(&'static str),
    /// Array-index path segments are not supported by this port.
    UnsupportedArrayIndex,
}

impl std::fmt::Display for ModifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModifyError::DeleteInEmptyDocument => write!(f, "can not delete in empty document"),
            ModifyError::AddPropertyTo(kind) => {
                write!(f, "can not add property to parent of type {kind}")
            }
            ModifyError::UnsupportedArrayIndex => {
                write!(f, "array index path segments are unsupported")
            }
        }
    }
}

/// Sets (`Some`) or removes (`None`) the value at `path` inside the JSONC
/// `text`, preserving comments, formatting, and every unrelated byte.
///
/// Returns `Ok(Some(updated))` with the new text, `Ok(None)` when a
/// removal target is absent (no edit — matching `applyEdits(text, [])`),
/// or `Err` for the shapes v2 throws on. Missing intermediate objects on
/// the path are created (jsonc `setProperty`'s value-wrapping walk); the
/// new value is spliced as compact JSON and the touched lines are then
/// re-indented by the range formatter, exactly like
/// `modify(text, path, value, { formattingOptions: { tabSize: 2,
/// insertSpaces: true } })`.
pub fn set_path(
    text: &str,
    path: &[&str],
    value: Option<&Value>,
) -> Result<Option<String>, ModifyError> {
    let tree = Tree::parse(text);
    let root = tree.root();

    // setProperty's walk: pop segments from the end until a parent exists,
    // wrapping the value in `{ segment: value }` for every missing level.
    let mut wrapped: Option<Value> = None;
    let mut last_segment = "";
    let mut parent: Option<usize> = None;
    for depth in (0..path.len()).rev() {
        last_segment = path[depth];
        parent = root.and_then(|root| tree.find(root, &path[..depth]));
        match (parent, wrapped.as_ref().or(value)) {
            (None, Some(current)) => {
                wrapped = Some(Value::Object(
                    [(path[depth].to_owned(), current.clone())].into_iter().collect(),
                ));
            }
            _ => break,
        }
    }
    let running: Option<&Value> = wrapped.as_ref().or(value);

    let Some(parent) = parent else {
        // No parent anywhere: empty document. Replace the root span (or
        // the whole empty text) with the (wrapped) value, then format.
        let Some(running) = running else {
            return Err(ModifyError::DeleteInEmptyDocument);
        };
        let (offset, length) = match root {
            Some(root) => (tree.nodes[root].offset, tree.nodes[root].length),
            None => (0, 0),
        };
        return Ok(Some(with_formatting(
            text,
            &Edit { offset, length, content: compact(running) },
        )));
    };

    let parent_kind = tree.nodes[parent].kind;
    if parent_kind != Kind::Object {
        if parent_kind == Kind::Array {
            return Err(ModifyError::UnsupportedArrayIndex);
        }
        return Err(ModifyError::AddPropertyTo(kind_name(parent_kind)));
    }

    let children = tree.nodes[parent].children.clone();
    if let Some(existing) = tree.find(parent, &[last_segment]) {
        match running {
            None => {
                // Delete: from the end of the previous property (its comma
                // rides along) or from just after `{` (through the comma of
                // the next property when deleting the first of many).
                let property = tree.nodes[existing].parent;
                let property_index =
                    children.iter().position(|child| *child == property).unwrap_or(0);
                let property_end = tree.nodes[property].offset + tree.nodes[property].length;
                let (remove_begin, remove_end) = if property_index > 0 {
                    let previous = children[property_index - 1];
                    (tree.nodes[previous].offset + tree.nodes[previous].length, property_end)
                } else {
                    let mut end = property_end;
                    if children.len() > 1 {
                        end = tree.nodes[children[1]].offset;
                    }
                    (tree.nodes[parent].offset + 1, end)
                };
                Ok(Some(with_formatting(
                    text,
                    &Edit {
                        offset: remove_begin,
                        length: remove_end - remove_begin,
                        content: String::new(),
                    },
                )))
            }
            Some(running) => {
                // Replace the existing value's span with the compact value.
                let node = &tree.nodes[existing];
                Ok(Some(with_formatting(
                    text,
                    &Edit { offset: node.offset, length: node.length, content: compact(running) },
                )))
            }
        }
    } else {
        let Some(running) = running else {
            return Ok(None); // property absent, nothing to do
        };
        // Insert `"key": <value>` after the last property (comma
        // prefixed) or bare just inside an empty object.
        let new_property =
            format!("{}: {}", compact(&Value::String(last_segment.to_owned())), compact(running));
        let edit = match children.last() {
            Some(&last_child) => {
                let end = tree.nodes[last_child].offset + tree.nodes[last_child].length;
                Edit { offset: end, length: 0, content: format!(",{new_property}") }
            }
            None => {
                Edit { offset: tree.nodes[parent].offset + 1, length: 0, content: new_property }
            }
        };
        Ok(Some(with_formatting(text, &edit)))
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Object | Kind::Property => "object",
        Kind::Array => "array",
        Kind::Str => "string",
        Kind::Num => "number",
        Kind::Bool => "boolean",
        Kind::Null => "null",
    }
}

/// Compact JSON serialization (JS `JSON.stringify`).
fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

/// A single text splice (jsonc-parser `Edit`).
struct Edit {
    offset: usize,
    length: usize,
    content: String,
}

fn apply_edit(text: &str, edit: &Edit) -> String {
    let mut spliced = String::with_capacity(text.len() + edit.content.len());
    spliced.push_str(&text[..edit.offset]);
    spliced.push_str(&edit.content);
    spliced.push_str(&text[edit.offset + edit.length..]);
    spliced
}

/// `withFormatting` with the fixed v2 options (`tabSize: 2, insertSpaces:
/// true`, `keepLines: false`): apply the edit, then re-format the touched
/// lines (inserts/removals expand to full lines; replacements format only
/// the new content's span).
fn with_formatting(text: &str, edit: &Edit) -> String {
    let mut new_text = apply_edit(text, edit);
    let mut begin = edit.offset;
    let mut end = edit.offset + edit.content.len();
    if edit.length == 0 || edit.content.is_empty() {
        while begin > 0 && !is_eol(new_text.as_bytes()[begin - 1]) {
            begin -= 1;
        }
        while end < new_text.len() && !is_eol(new_text.as_bytes()[end]) {
            end += 1;
        }
    }
    let edits = format_range(&new_text, begin, end);
    for edit in edits.iter().rev() {
        new_text = apply_edit(&new_text, edit);
    }
    new_text
}

/// The jsonc-parser formatter (`impl/format.js`) over `[begin, end)` of
/// `text`: normalizes the touched lines to 2-space indentation, `": "`
/// after colons, and one property/element per line. Comments are tokens —
/// kept and padded with a single space. Any scanner error aborts all
/// formatting (the raw splice stands), and edits that fall outside the
/// requested range are dropped.
#[allow(clippy::too_many_lines)] // faithful port of format()'s token walk
fn format_range(text: &str, range_start: usize, range_end: usize) -> Vec<Edit> {
    struct Formatter<'a> {
        text: &'a str,
        range_start: usize,
        range_end: usize,
        format_text_start: usize,
        eol: &'static str,
        initial_indent_level: usize,
        indent_level: isize,
        number_line_breaks: bool,
        has_error: bool,
        edits: Vec<Edit>,
        scan: Scanner<'a>,
    }

    impl Formatter<'_> {
        fn new_lines_and_indent(&self) -> String {
            let total = self.initial_indent_level.saturating_add_signed(self.indent_level);
            let mut indented = String::with_capacity(self.eol.len() + 2 * total);
            indented.push_str(self.eol);
            for _ in 0..total {
                indented.push_str("  ");
            }
            indented
        }

        fn add_edit(&mut self, content: &str, start: usize, end: usize) {
            if !self.has_error
                && start < self.range_end
                && end > self.range_start
                && &self.text[start..end] != content
            {
                self.edits.push(Edit {
                    offset: start,
                    length: end - start,
                    content: content.to_owned(),
                });
            }
        }

        fn token_start(&self) -> usize {
            self.scan.token_offset + self.format_text_start
        }

        fn token_end(&self) -> usize {
            self.scan.token_offset + self.scan.token_len() + self.format_text_start
        }

        fn scan_next(&mut self) -> Tok {
            let mut token = self.scan.scan();
            self.number_line_breaks = false;
            while token == Tok::Trivia || token == Tok::LineBreak {
                if token == Tok::LineBreak {
                    self.number_line_breaks = true;
                }
                token = self.scan.scan();
            }
            self.has_error = token == Tok::Unknown || self.scan.token_error;
            token
        }
    }

    let bytes = text.as_bytes();
    let mut format_text_start = range_start;
    while format_text_start > 0 && !is_eol(bytes[format_text_start - 1]) {
        format_text_start -= 1;
    }
    let mut end_offset = range_end;
    while end_offset < text.len() && !is_eol(bytes[end_offset]) {
        end_offset += 1;
    }
    let format_text = &text[format_text_start..end_offset];

    let mut fmt = Formatter {
        text,
        range_start,
        range_end,
        format_text_start,
        eol: detect_eol(text),
        initial_indent_level: compute_indent_level(format_text),
        indent_level: 0,
        number_line_breaks: false,
        has_error: false,
        edits: Vec::new(),
        scan: Scanner::new(format_text),
    };

    let mut first_token = fmt.scan_next();
    if first_token != Tok::Eof {
        let initial_indent = "  ".repeat(fmt.initial_indent_level);
        let first_token_start = fmt.token_start();
        fmt.add_edit(&initial_indent, format_text_start, first_token_start);
    }
    while first_token != Tok::Eof {
        let mut first_token_end = fmt.token_end();
        let mut second_token = fmt.scan_next();
        let mut replace_content = String::new();
        let mut needs_line_break = false;
        // Same-line comments between tokens: pad with one space; a line
        // comment forces the following break/indent.
        while !fmt.number_line_breaks
            && matches!(second_token, Tok::LineComment | Tok::BlockComment)
        {
            let comment_start = fmt.token_start();
            fmt.add_edit(" ", first_token_end, comment_start);
            first_token_end = fmt.token_end();
            needs_line_break = second_token == Tok::LineComment;
            replace_content =
                if needs_line_break { fmt.new_lines_and_indent() } else { String::new() };
            second_token = fmt.scan_next();
        }
        if second_token == Tok::CloseBrace {
            if first_token != Tok::OpenBrace {
                fmt.indent_level -= 1;
                replace_content = fmt.new_lines_and_indent();
            }
        } else if second_token == Tok::CloseBracket {
            if first_token != Tok::OpenBracket {
                fmt.indent_level -= 1;
                replace_content = fmt.new_lines_and_indent();
            }
        } else {
            match first_token {
                Tok::OpenBracket | Tok::OpenBrace => {
                    fmt.indent_level += 1;
                    replace_content = fmt.new_lines_and_indent();
                }
                Tok::Comma | Tok::LineComment => replace_content = fmt.new_lines_and_indent(),
                Tok::BlockComment => {
                    if fmt.number_line_breaks {
                        replace_content = fmt.new_lines_and_indent();
                    } else if !needs_line_break {
                        replace_content = String::from(" ");
                    }
                }
                Tok::Colon => {
                    if !needs_line_break {
                        replace_content = String::from(" ");
                    }
                }
                Tok::StringLiteral => {
                    if second_token == Tok::Colon && !needs_line_break {
                        replace_content = String::new();
                    }
                }
                Tok::NullKeyword
                | Tok::TrueKeyword
                | Tok::FalseKeyword
                | Tok::NumericLiteral
                | Tok::CloseBrace
                | Tok::CloseBracket => {
                    if matches!(second_token, Tok::LineComment | Tok::BlockComment)
                        && !needs_line_break
                    {
                        replace_content = String::from(" ");
                    } else if second_token != Tok::Comma && second_token != Tok::Eof {
                        fmt.has_error = true;
                    }
                }
                Tok::Unknown => fmt.has_error = true,
                _ => {}
            }
            if fmt.number_line_breaks
                && matches!(second_token, Tok::LineComment | Tok::BlockComment)
            {
                replace_content = fmt.new_lines_and_indent();
            }
        }
        if second_token == Tok::Eof {
            replace_content = String::new();
        }
        let second_token_start = fmt.token_start();
        fmt.add_edit(&replace_content, first_token_end, second_token_start);
        first_token = second_token;
    }
    fmt.edits
}

fn compute_indent_level(text: &str) -> usize {
    let mut chars = 0;
    for byte in text.bytes() {
        if byte == b' ' {
            chars += 1;
        } else if byte == b'\t' {
            chars += 2;
        } else {
            break;
        }
    }
    chars / 2
}

fn detect_eol(text: &str) -> &'static str {
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'\r' => return if bytes.get(index + 1) == Some(&b'\n') { "\r\n" } else { "\r" },
            b'\n' => return "\n",
            _ => {}
        }
    }
    "\n"
}

fn is_eol(byte: u8) -> bool {
    byte == b'\r' || byte == b'\n'
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Tok {
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    Comma,
    Colon,
    NullKeyword,
    TrueKeyword,
    FalseKeyword,
    StringLiteral,
    NumericLiteral,
    LineComment,
    BlockComment,
    LineBreak,
    Trivia,
    Unknown,
    Eof,
}

/// Byte-offset JSONC scanner (jsonc-parser `impl/scanner.js`). Structural
/// characters are ASCII, so every token boundary is a char boundary and
/// byte offsets splice safely.
struct Scanner<'a> {
    text: &'a str,
    pos: usize,
    token: Tok,
    token_offset: usize,
    token_error: bool,
    value: String,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Scanner {
            text,
            pos: 0,
            token: Tok::Unknown,
            token_offset: 0,
            token_error: false,
            value: String::new(),
        }
    }

    fn token_len(&self) -> usize {
        self.pos - self.token_offset
    }

    #[allow(clippy::too_many_lines)] // faithful port of jsonc scan()'s token ladder
    fn scan(&mut self) -> Tok {
        self.value.clear();
        self.token_error = false;
        self.token_offset = self.pos;
        let bytes = self.text.as_bytes();
        let len = bytes.len();
        if self.pos >= len {
            self.token_offset = len;
            self.token = Tok::Eof;
            return self.token;
        }
        let code = bytes[self.pos];
        if is_ws(code) {
            loop {
                self.pos += 1;
                if self.pos >= len || !is_ws(bytes[self.pos]) {
                    break;
                }
            }
            self.token = Tok::Trivia;
            return self.token;
        }
        if is_lb(code) {
            self.pos += 1;
            if code == b'\r' && self.pos < len && bytes[self.pos] == b'\n' {
                self.pos += 1;
            }
            self.token = Tok::LineBreak;
            return self.token;
        }
        self.token = match code {
            b'{' => {
                self.pos += 1;
                Tok::OpenBrace
            }
            b'}' => {
                self.pos += 1;
                Tok::CloseBrace
            }
            b'[' => {
                self.pos += 1;
                Tok::OpenBracket
            }
            b']' => {
                self.pos += 1;
                Tok::CloseBracket
            }
            b':' => {
                self.pos += 1;
                Tok::Colon
            }
            b',' => {
                self.pos += 1;
                Tok::Comma
            }
            b'"' => {
                self.pos += 1;
                self.scan_string();
                Tok::StringLiteral
            }
            b'/' if bytes.get(self.pos + 1) == Some(&b'/') => {
                self.pos += 2;
                while self.pos < len && !is_lb(bytes[self.pos]) {
                    self.pos += 1;
                }
                Tok::LineComment
            }
            b'/' if bytes.get(self.pos + 1) == Some(&b'*') => {
                self.pos += 2;
                let safe = len.saturating_sub(1);
                let mut closed = false;
                while self.pos < safe {
                    let current = bytes[self.pos];
                    if current == b'*' && bytes[self.pos + 1] == b'/' {
                        self.pos += 2;
                        closed = true;
                        break;
                    }
                    self.pos += 1;
                    if current == b'\r' && self.pos < len && bytes[self.pos] == b'\n' {
                        self.pos += 1;
                    }
                }
                if !closed {
                    self.pos += 1;
                    self.token_error = true;
                }
                Tok::BlockComment
            }
            // A bare slash, or a minus not followed by a digit: an unknown
            // single-char token.
            b'/' | b'-'
                if code == b'/' || !is_digit(bytes.get(self.pos + 1).copied().unwrap_or(b'0')) =>
            {
                self.pos += 1;
                Tok::Unknown
            }
            // A minus followed by a digit scans a number.
            b'-' => {
                self.pos += 1;
                self.scan_number();
                Tok::NumericLiteral
            }
            b'0'..=b'9' => {
                self.scan_number();
                Tok::NumericLiteral
            }
            _ => {
                while self.pos < len && is_unknown_content(bytes[self.pos]) {
                    self.pos += 1;
                }
                if self.pos > self.token_offset {
                    match &self.text[self.token_offset..self.pos] {
                        "true" => Tok::TrueKeyword,
                        "false" => Tok::FalseKeyword,
                        "null" => Tok::NullKeyword,
                        _ => Tok::Unknown,
                    }
                } else {
                    self.pos += 1;
                    Tok::Unknown
                }
            }
        };
        self.token
    }

    /// Decodes a string literal body (jsonc `scanString`): escapes resolve;
    /// unterminated or invalid escapes set `token_error` but still yield a
    /// token.
    fn scan_string(&mut self) {
        let bytes = self.text.as_bytes();
        let len = bytes.len();
        let mut result = String::new();
        let mut start = self.pos;
        loop {
            if self.pos >= len {
                result.push_str(&self.text[start..self.pos]);
                self.token_error = true;
                break;
            }
            let current = bytes[self.pos];
            if current == b'"' {
                result.push_str(&self.text[start..self.pos]);
                self.pos += 1;
                break;
            }
            if current == b'\\' {
                result.push_str(&self.text[start..self.pos]);
                self.pos += 1;
                if self.pos >= len {
                    self.token_error = true;
                    break;
                }
                let escaped = bytes[self.pos];
                self.pos += 1;
                match escaped {
                    b'"' => result.push('"'),
                    b'\\' => result.push('\\'),
                    b'/' => result.push('/'),
                    b'b' => result.push('\u{8}'),
                    b'f' => result.push('\u{c}'),
                    b'n' => result.push('\n'),
                    b'r' => result.push('\r'),
                    b't' => result.push('\t'),
                    b'u' => match self.scan_hex4() {
                        Some(code) => result.push(char::from_u32(code).unwrap_or('\u{fffd}')),
                        None => self.token_error = true,
                    },
                    _ => self.token_error = true,
                }
                start = self.pos;
                continue;
            }
            if current < 0x20 {
                if is_lb(current) {
                    result.push_str(&self.text[start..self.pos]);
                    self.token_error = true;
                    break;
                }
                self.token_error = true;
            }
            self.pos += 1;
        }
        self.value = result;
    }

    fn scan_hex4(&mut self) -> Option<u32> {
        let bytes = self.text.as_bytes();
        let mut value: u32 = 0;
        for _ in 0..4 {
            let digit = bytes.get(self.pos).copied()?;
            let nibble = match digit {
                b'0'..=b'9' => u32::from(digit - b'0'),
                b'A'..=b'F' => u32::from(digit - b'A') + 10,
                b'a'..=b'f' => u32::from(digit - b'a') + 10,
                _ => return None,
            };
            value = value * 16 + nibble;
            self.pos += 1;
        }
        Some(value)
    }

    /// Token extents mirror jsonc `scanNumber` (the numeric text itself is
    /// unused downstream; only the span and error flag matter).
    fn scan_number(&mut self) {
        let bytes = self.text.as_bytes();
        let len = bytes.len();
        if bytes[self.pos] == b'0' {
            self.pos += 1;
        } else {
            self.pos += 1;
            while self.pos < len && is_digit(bytes[self.pos]) {
                self.pos += 1;
            }
        }
        if self.pos < len && bytes[self.pos] == b'.' {
            self.pos += 1;
            if self.pos < len && is_digit(bytes[self.pos]) {
                self.pos += 1;
                while self.pos < len && is_digit(bytes[self.pos]) {
                    self.pos += 1;
                }
            } else {
                self.token_error = true;
                return;
            }
        }
        if self.pos < len && (bytes[self.pos] == b'e' || bytes[self.pos] == b'E') {
            self.pos += 1;
            if self.pos < len && (bytes[self.pos] == b'+' || bytes[self.pos] == b'-') {
                self.pos += 1;
            }
            if self.pos < len && is_digit(bytes[self.pos]) {
                self.pos += 1;
                while self.pos < len && is_digit(bytes[self.pos]) {
                    self.pos += 1;
                }
            } else {
                self.token_error = true;
            }
        }
    }
}

fn is_ws(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

fn is_lb(byte: u8) -> bool {
    byte == b'\n' || byte == b'\r'
}

fn is_digit(byte: u8) -> bool {
    byte.is_ascii_digit()
}

fn is_unknown_content(byte: u8) -> bool {
    !is_ws(byte)
        && !is_lb(byte)
        && !matches!(byte, b'}' | b']' | b'{' | b'[' | b'"' | b':' | b',' | b'/')
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Object,
    Array,
    Property,
    Str,
    Num,
    Bool,
    Null,
}

struct Node {
    kind: Kind,
    offset: usize,
    length: usize,
    colon_offset: Option<usize>,
    children: Vec<usize>,
    /// Decoded string content (property keys, string literals).
    value: Option<String>,
    parent: usize,
}

/// Tolerant JSONC tree (jsonc-parser `parseTree`): parse errors never
/// abort; best-effort node spans drive the splicer. Incomplete properties
/// (missing value) keep their key-only children so `find` skips them,
/// exactly like `findNodeAtLocation`'s `children.length === 2` check.
struct Tree {
    nodes: Vec<Node>,
}

const ARTIFICIAL: usize = 0;
const NO_PARENT: usize = usize::MAX;

impl Tree {
    fn parse(text: &str) -> Tree {
        let mut tree = Tree {
            nodes: vec![Node {
                kind: Kind::Array,
                offset: 0,
                length: 0,
                colon_offset: None,
                children: Vec::new(),
                value: None,
                parent: NO_PARENT,
            }],
        };
        let mut parser = Parser { scan: Scanner::new(text), tree: &mut tree, current: ARTIFICIAL };
        parser.scan_next();
        if parser.scan.token == Tok::Eof {
            return tree; // empty (or comments-only) document
        }
        parser.parse_value();
        tree
    }

    fn root(&self) -> Option<usize> {
        self.nodes[ARTIFICIAL].children.first().copied()
    }

    /// `findNodeAtLocation` for string segments: the first property whose
    /// key matches, returning its value node.
    fn find(&self, mut node: usize, path: &[&str]) -> Option<usize> {
        for segment in path {
            if self.nodes[node].kind != Kind::Object {
                return None;
            }
            let mut found = None;
            for &property in &self.nodes[node].children {
                let property_node = &self.nodes[property];
                if property_node.children.len() == 2
                    && self.nodes[property_node.children[0]].value.as_deref() == Some(*segment)
                {
                    found = Some(property_node.children[1]);
                    break;
                }
            }
            node = found?;
        }
        Some(node)
    }
}

struct Parser<'a, 'b> {
    scan: Scanner<'a>,
    tree: &'b mut Tree,
    current: usize,
}

impl Parser<'_, '_> {
    /// The parser's `scanNext`: skip whitespace, line breaks, comments,
    /// and unknown tokens — parse errors are recorded upstream and
    /// ignored by `setProperty`.
    fn scan_next(&mut self) -> Tok {
        loop {
            let token = self.scan.scan();
            match token {
                Tok::Trivia
                | Tok::LineBreak
                | Tok::LineComment
                | Tok::BlockComment
                | Tok::Unknown => {}
                _ => return token,
            }
        }
    }

    /// `handleError`'s skip lists: resynchronize at the next token in
    /// `until_after` (consuming it) or `until` (leaving it).
    fn recover(&mut self, until_after: &[Tok], until: &[Tok]) {
        if until_after.is_empty() && until.is_empty() {
            return;
        }
        let mut token = self.scan.token;
        while token != Tok::Eof {
            if until_after.contains(&token) {
                self.scan_next();
                break;
            }
            if until.contains(&token) {
                break;
            }
            token = self.scan_next();
        }
    }

    fn push_node(&mut self, kind: Kind, offset: usize) -> usize {
        let node = Node {
            kind,
            offset,
            length: 0,
            colon_offset: None,
            children: Vec::new(),
            value: None,
            parent: self.current,
        };
        self.tree.nodes.push(node);
        let index = self.tree.nodes.len() - 1;
        self.tree.nodes[self.current].children.push(index);
        index
    }

    fn ensure_property_complete(&mut self, end: usize) {
        if self.tree.nodes[self.current].kind == Kind::Property {
            let property = self.current;
            let start = self.tree.nodes[property].offset;
            self.tree.nodes[property].length = end.saturating_sub(start);
            self.current = self.tree.nodes[property].parent;
        }
    }

    /// `onObjectEnd`/`onArrayEnd`: close the current container at the
    /// current token's end (closing brace/bracket, or EOF for an
    /// unterminated container).
    fn end_container(&mut self) {
        let end = self.scan.token_offset + self.scan.token_len();
        self.ensure_property_complete(end);
        let node = self.current;
        let start = self.tree.nodes[node].offset;
        self.tree.nodes[node].length = end.saturating_sub(start);
        self.current = self.tree.nodes[node].parent;
        self.ensure_property_complete(end);
    }

    fn parse_value(&mut self) -> bool {
        match self.scan.token {
            Tok::OpenBracket => self.parse_array(),
            Tok::OpenBrace => self.parse_object(),
            Tok::StringLiteral => {
                self.parse_string_literal();
                true
            }
            token @ (Tok::NumericLiteral
            | Tok::NullKeyword
            | Tok::TrueKeyword
            | Tok::FalseKeyword) => {
                let kind = match token {
                    Tok::NumericLiteral => Kind::Num,
                    Tok::NullKeyword => Kind::Null,
                    _ => Kind::Bool,
                };
                let offset = self.scan.token_offset;
                let length = self.scan.token_len();
                let node = self.push_node(kind, offset);
                self.tree.nodes[node].length = length;
                self.scan_next();
                self.ensure_property_complete(offset + length);
                true
            }
            _ => false,
        }
    }

    fn parse_string_literal(&mut self) {
        let offset = self.scan.token_offset;
        let length = self.scan.token_len();
        let value = self.scan.value.clone();
        let node = self.push_node(Kind::Str, offset);
        self.tree.nodes[node].length = length;
        self.tree.nodes[node].value = Some(value);
        self.scan_next();
        self.ensure_property_complete(offset + length);
    }

    fn parse_object(&mut self) -> bool {
        let object = self.push_node(Kind::Object, self.scan.token_offset);
        self.current = object;
        self.scan_next();
        while self.scan.token != Tok::CloseBrace && self.scan.token != Tok::Eof {
            if self.scan.token == Tok::Comma {
                // onSeparator(','): closes a property dangling after a
                // comma. (`allowTrailingComma` is false in setProperty's
                // parse — the loop simply continues past a trailing comma.)
                self.ensure_property_complete(self.scan.token_offset);
                self.scan_next();
            }
            if !self.parse_property() {
                // ValueExpected: resync AT `}` or `,` (unconsumed).
                self.recover(&[], &[Tok::CloseBrace, Tok::Comma]);
            }
        }
        self.end_container();
        if self.scan.token == Tok::CloseBrace {
            self.scan_next();
        } else {
            // CloseBraceExpected: skip forward past the `}` to resume.
            self.recover(&[Tok::CloseBrace], &[]);
        }
        true
    }

    fn parse_property(&mut self) -> bool {
        if self.scan.token != Tok::StringLiteral {
            // PropertyNameExpected: resync AT `}` or `,` (unconsumed).
            self.recover(&[], &[Tok::CloseBrace, Tok::Comma]);
            return false;
        }
        let key_offset = self.scan.token_offset;
        let key_length = self.scan.token_len();
        let key = self.scan.value.clone();
        let property = self.push_node(Kind::Property, key_offset);
        self.current = property;
        let key_node = Node {
            kind: Kind::Str,
            offset: key_offset,
            length: key_length,
            colon_offset: None,
            children: Vec::new(),
            value: Some(key),
            parent: property,
        };
        self.tree.nodes.push(key_node);
        let key_index = self.tree.nodes.len() - 1;
        self.tree.nodes[property].children.push(key_index);
        self.scan_next();
        if self.scan.token == Tok::Colon {
            self.tree.nodes[property].colon_offset = Some(self.scan.token_offset);
            self.scan_next();
            if !self.parse_value() {
                self.recover(&[], &[Tok::CloseBrace, Tok::Comma]);
            }
        } else {
            self.recover(&[], &[Tok::CloseBrace, Tok::Comma]);
        }
        true
    }

    fn parse_array(&mut self) -> bool {
        let array = self.push_node(Kind::Array, self.scan.token_offset);
        self.current = array;
        self.scan_next();
        while self.scan.token != Tok::CloseBracket && self.scan.token != Tok::Eof {
            if self.scan.token == Tok::Comma {
                self.ensure_property_complete(self.scan.token_offset);
                self.scan_next();
            }
            if !self.parse_value() {
                // ValueExpected: resync AT `]` or `,` (unconsumed).
                self.recover(&[], &[Tok::CloseBracket, Tok::Comma]);
            }
        }
        self.end_container();
        if self.scan.token == Tok::CloseBracket {
            self.scan_next();
        } else {
            self.recover(&[Tok::CloseBracket], &[]);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Golden corpus generated with the real jsonc-parser@3.3.1
    /// (`modify` + `applyEdits`, `tabSize: 2, insertSpaces: true`) — the
    /// exact write path of every v2 config writer. Values are key-sorted
    /// before the reference run to mirror the map ordering of `serde_json` (the
    /// registered divergence; only `replace-nested` differs from v2's
    /// insertion-order output).
    #[test]
    fn corpus_matches_jsonc_parser_bytes() {
        let corpus: serde_json::Value =
            serde_json::from_str(include_str!("jsonc_edit_fixtures.json")).expect("fixtures parse");
        let cases = corpus.as_object().expect("object");
        assert!(cases.len() >= 30, "corpus shrank: {}", cases.len());
        for (name, case) in cases {
            let input = case["input"].as_str().expect("input");
            let path: Vec<&str> = case["path"]
                .as_array()
                .expect("path array")
                .iter()
                .map(|segment| segment.as_str().expect("path segment"))
                .collect::<Vec<_>>();
            let value = &case["value"];
            let expected = case["output"].as_str().expect("output");
            // `null` encodes the removal (jsonc modify's `undefined`);
            // otherwise the value is JSON text to parse (so `"null"` sets
            // a null value while JSON `null` deletes).
            let result = if value.is_null() {
                set_path(input, &path, None)
            } else {
                let parsed: Value = serde_json::from_str(value.as_str().expect("value json text"))
                    .expect("value json");
                set_path(input, &path, Some(&parsed))
            };
            let updated = result.unwrap_or_else(|error| panic!("{name}: editor error {error:?}"));
            let updated = updated.unwrap_or_else(|| input.to_owned());
            assert_eq!(updated, expected, "case {name} diverged from jsonc-parser");
        }
    }

    /// The result must still parse to the mutated value (comments and all
    /// tolerated by json5, the same tolerance class the loader uses).
    #[test]
    fn results_parse_to_expected_values() {
        let checks: &[(&str, Vec<&str>, Option<Value>, Value)] = &[
            (
                "{\n  // annotated\n  \"username\": \"u\"\n}\n",
                vec!["mcp", "servers", "github"],
                Some(json!({"type": "remote", "url": "https://x"})),
                json!({"username": "u", "mcp": {"servers": {"github": {"type": "remote", "url": "https://x"}}}}),
            ),
            (
                "{\n  /* keep */\n  \"shell\": \"old\",\n  \"x\": 1\n}",
                vec!["shell"],
                Some(json!("/bin/new")),
                json!({"shell": "/bin/new", "x": 1}),
            ),
            ("{\n  \"shell\": \"old\",\n  \"x\": 1\n}", vec!["shell"], None, json!({"x": 1})),
        ];
        for (input, path, value, expected) in checks {
            let updated =
                set_path(input, path, value.as_ref()).expect("edit applies").expect("changed");
            let parsed: Value = json5::from_str(&updated).expect("result parses");
            assert_eq!(&parsed, expected, "input: {input}");
        }
    }

    /// Untouched regions stay byte-identical: the edit is a splice, so the
    /// text before the splice start and after the splice end survives
    /// verbatim (modulo the formatter's same-line normalization).
    #[test]
    fn untouched_regions_survive_byte_for_byte() {
        let input = "{\n  // header comment\n  \"username\": \"u\",\n  \"theme\": \"dark\"\n}\n";
        let updated = set_path(input, &["mcp"], Some(&json!({"servers": {}})))
            .expect("edit")
            .expect("changed");
        // The inserted property is appended after the last property
        // ("theme"), so the prefix through `"theme": "dark"` is
        // byte-identical.
        let marker = "\"theme\": \"dark\"";
        let index = updated.find(marker).expect("theme kept");
        assert_eq!(
            &updated[..index + marker.len()],
            "{\n  // header comment\n  \"username\": \"u\",\n  \"theme\": \"dark\"",
            "prefix untouched"
        );
        assert!(updated.ends_with(",\n  \"mcp\": {\n    \"servers\": {}\n  }\n}\n"));
        assert!(updated.contains("// header comment"), "comment verbatim");
    }

    /// Three pinned before→after byte pairs (the shapes the D-01 writers
    /// produce most often).
    #[test]
    fn golden_pairs() {
        // mcp add into a commented config (v2 mcp/add.ts).
        let mcp = set_path(
            "{\n  // annotated\n  \"username\": \"u\"\n}\n",
            &["mcp", "servers", "github"],
            Some(&json!({"url": "https://mcp.example.com/sse", "type": "remote"})),
        )
        .expect("edit")
        .expect("changed");
        assert_eq!(
            mcp,
            "{\n  // annotated\n  \"username\": \"u\",\n  \"mcp\": {\n    \"servers\": {\n      \"github\": {\n        \"type\": \"remote\",\n        \"url\": \"https://mcp.example.com/sse\"\n      }\n    }\n  }\n}\n"
        );

        // plugin add: set ["plugins"] to the whole new array.
        let plugins = set_path(
            "{\n  // plugins live here\n  \"plugins\": [\n    \"a\",\n  ],\n}\n",
            &["plugins"],
            Some(&json!(["a", "b"])),
        )
        .expect("edit")
        .expect("changed");
        assert_eq!(
            plugins,
            "{\n  // plugins live here\n  \"plugins\": [\n    \"a\",\n    \"b\"\n  ],\n}\n"
        );

        // Config.update shell removal (v2 passes undefined → remove).
        let removed = set_path("{\n  \"shell\": \"/bin/old\"\n}\n", &["shell"], None)
            .expect("edit")
            .expect("changed");
        assert_eq!(removed, "{\n}\n");
    }

    /// Registered edge: removing an absent key is a no-op (empty edit
    /// list), matching `applyEdits(text, [])`.
    #[test]
    fn remove_absent_key_is_noop() {
        assert_eq!(
            set_path("{\n  \"a\": 1\n}\n", &["zz"], None).expect("ok"),
            None,
            "no edit produced"
        );
    }

    /// The error shapes v2 throws on.
    #[test]
    fn error_shapes_match_v2() {
        // Delete against a comments-only document.
        assert_eq!(
            set_path("// only a comment\n", &["a"], None).unwrap_err(),
            ModifyError::DeleteInEmptyDocument
        );
        // Parent is an array root.
        assert_eq!(
            set_path("[\n  1\n]\n", &["a"], Some(&json!(1))).unwrap_err(),
            ModifyError::UnsupportedArrayIndex
        );
        // Parent is a scalar root.
        assert_eq!(
            set_path("\"scalar\"\n", &["a"], Some(&json!(1))).unwrap_err(),
            ModifyError::AddPropertyTo("string")
        );
    }

    /// CRLF files keep their line endings (eol detection scans the whole
    /// document).
    #[test]
    fn crlf_line_endings_preserved() {
        let updated = set_path("{\r\n  // c\r\n  \"a\": 1\r\n}\r\n", &["b"], Some(&json!(2)))
            .expect("edit")
            .expect("changed");
        assert_eq!(updated, "{\r\n  // c\r\n  \"a\": 1,\r\n  \"b\": 2\r\n}\r\n");
    }

    /// Empty document: the whole text is replaced by the formatted value
    /// (v2's empty-document branch; the port's writers gate on an object
    /// root before ever reaching it).
    #[test]
    fn empty_document_replaces_root() {
        let updated = set_path("", &["a"], Some(&json!({"b": 1}))).expect("edit").expect("changed");
        assert_eq!(updated, "{\n  \"a\": {\n    \"b\": 1\n  }\n}");
    }

    /// Comments on the touched line survive as tokens (single-space
    /// padding), including trailing comments after the inserted property.
    #[test]
    fn comments_on_touched_lines_survive() {
        let updated = set_path("{\n  \"a\": 1 // note\n}\n", &["b"], Some(&json!(2)))
            .expect("edit")
            .expect("changed");
        assert_eq!(updated, "{\n  \"a\": 1,\n  \"b\": 2 // note\n}\n");
    }

    /// Registered v2 behavior kept verbatim: deleting a property also
    /// removes anything between it and the neighbor (here a comment line).
    #[test]
    fn delete_span_includes_interleaved_comment() {
        let updated = set_path(
            "{\n  \"shell\": \"/bin/old\",\n  // keep\n  \"username\": \"u\"\n}\n",
            &["shell"],
            None,
        )
        .expect("edit")
        .expect("changed");
        assert_eq!(updated, "{\n  \"username\": \"u\"\n}\n");
    }
}
