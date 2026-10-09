//! Port of `packages/core/src/tool/html-markdown.ts` plus the small
//! htmlparser2 surface it uses (streaming tokenizer with entity decoding,
//! void-element auto-close, implied end tags for `p`/`li`/`dt`/`dd`/`td`).
//!
// Fidelity-first port of html-markdown.ts + the htmlparser2 subset.
#![allow(
    clippy::must_use_candidate,
    clippy::needless_return,
    clippy::match_same_arms,
    clippy::if_same_then_else,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::option_map_unit_fn,
    clippy::single_match,
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    clippy::manual_map,
    clippy::redundant_closure,
    clippy::struct_excessive_bools,
    clippy::cast_possible_truncation,
    clippy::redundant_field_names,
    clippy::unnecessary_lazy_evaluations,
    clippy::ptr_arg,
    clippy::assigning_clones,
    clippy::single_char_pattern
)]

//! Byte budgets and every escape rule are byte-exact against v2; the tests
//! port `tool-webfetch.test.ts`'s converter cases.

use std::cell::RefCell;
use std::rc::Rc;

const MAX_MARKDOWN_BYTES: usize = 5 * 1024 * 1024;
const CONTENT_BYTES: usize = MAX_MARKDOWN_BYTES - 64 * 1024;

const OMITTED: &[&str] =
    &["script", "style", "noscript", "iframe", "object", "embed", "meta", "link", "template"];
const BLOCKS: &[&str] = &[
    "address",
    "article",
    "aside",
    "details",
    "dialog",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "header",
    "main",
    "nav",
    "p",
    "section",
    "summary",
];
const VOID: &[&str] = &[
    "area", "base", "br", "col", "command", "embed", "hr", "img", "input", "keygen", "link",
    "meta", "param", "source", "track", "wbr",
];
const RAW_TEXT: &[&str] = &["script", "style", "title", "textarea", "plaintext"];

// ---------------------------------------------------------------------------
// Tokenizer (htmlparser2 subset)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct OpenTag {
    name: String,
    attributes: Vec<(String, String)>,
    /// Parity surface: htmlparser2's tokenizer surfaces the self-closing
    /// flag on every open tag; kept for the raw-text/foreign-content phases.
    #[allow(dead_code)]
    self_closing: bool,
}

#[derive(Debug, Clone)]
enum Token {
    Open(OpenTag),
    Close(String),
    Text(String),
}

#[allow(dead_code)]
fn attr<'a>(open: &'a OpenTag, name: &str) -> Option<&'a str> {
    open.attributes.iter().find(|(key, _)| key.as_str() == name).map(|(_, value)| value.as_str())
}

/// Parity surface: htmlparser2 `hasAttribute` helper (kept alongside `attr`).
#[allow(dead_code)]
fn has_attr(open: &OpenTag, name: &str) -> bool {
    open.attributes.iter().any(|(key, _)| key == name)
}

/// Elements implied-closed by a new open tag or a close tag.
fn implies_end_of(opening: &str) -> &[&'static str] {
    match opening {
        "li" => &["li"],
        "dt" | "dd" => &["dt", "dd"],
        "p" => &["p"],
        "option" => &["option"],
        "tr" => &["td", "th", "tr"],
        "td" | "th" => &["td", "th"],
        "tbody" | "thead" | "tfoot" => &["td", "th", "tr", "tbody", "thead", "tfoot"],
        _ => &[],
    }
}

fn tokenize(html: &str) -> Vec<Token> {
    let chars: Vec<char> = html.chars().collect();
    let mut tokens = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let char = chars[index];
        if char != '<' {
            let start = index;
            while index < chars.len() && chars[index] != '<' {
                index += 1;
            }
            let text = decode_entities(&chars[start..index].iter().collect::<String>());
            if !text.is_empty() {
                tokens.push(Token::Text(text));
            }
            continue;
        }
        // Comments, doctypes, processing instructions.
        if starts_with(&chars, index, "<!--") {
            let end = find(&chars, index + 4, "-->");
            index = end.map_or(chars.len(), |end| end + 3);
            continue;
        }
        if starts_with(&chars, index, "<!") || starts_with(&chars, index, "<?") {
            let end = find(&chars, index, ">");
            index = end.map_or(chars.len(), |end| end + 1);
            continue;
        }
        if starts_with(&chars, index, "</") {
            let mut cursor = index + 2;
            while cursor < chars.len() && chars[cursor].is_alphanumeric() {
                cursor += 1;
            }
            let name: String = chars[index + 2..cursor].iter().collect::<String>().to_lowercase();
            if name.is_empty() {
                index += 2;
                continue;
            }
            let close = find(&chars, cursor, ">").map_or(chars.len(), |end| end + 1);
            index = close;
            // Implied end tags: close everything above the matching open
            // (emitting their closes), then drop the match itself — the
            // explicit Close(name) event below settles it.
            if let Some(depth) = stack.iter().rposition(|open| *open == name) {
                while stack.len() > depth + 1 {
                    tokens.push(Token::Close(stack.pop().unwrap_or_default()));
                }
                stack.pop();
            }
            tokens.push(Token::Close(name));
            continue;
        }
        // Open tag.
        let mut cursor = index + 1;
        let start = cursor;
        while cursor < chars.len() && (chars[cursor].is_alphanumeric() || chars[cursor] == '-') {
            cursor += 1;
        }
        if cursor == start {
            // Stray '<' — text.
            index += 1;
            let text = decode_entities("<");
            tokens.push(Token::Text(text));
            continue;
        }
        let name: String = chars[start..cursor].iter().collect::<String>().to_lowercase();
        let mut attributes = Vec::new();
        let mut self_closing = false;
        loop {
            while cursor < chars.len() && chars[cursor].is_whitespace() {
                cursor += 1;
            }
            if cursor >= chars.len() {
                break;
            }
            if chars[cursor] == '>' {
                cursor += 1;
                break;
            }
            if chars[cursor] == '/' && cursor + 1 < chars.len() && chars[cursor + 1] == '>' {
                self_closing = true;
                cursor += 2;
                break;
            }
            // attribute name
            let attr_start = cursor;
            while cursor < chars.len()
                && !chars[cursor].is_whitespace()
                && chars[cursor] != '='
                && chars[cursor] != '>'
                && chars[cursor] != '/'
            {
                cursor += 1;
            }
            if cursor == attr_start {
                cursor += 1;
                continue;
            }
            let attr_name: String =
                chars[attr_start..cursor].iter().collect::<String>().to_lowercase();
            while cursor < chars.len() && chars[cursor].is_whitespace() {
                cursor += 1;
            }
            let value = if cursor < chars.len() && chars[cursor] == '=' {
                cursor += 1;
                while cursor < chars.len() && chars[cursor].is_whitespace() {
                    cursor += 1;
                }
                if cursor < chars.len() && (chars[cursor] == '"' || chars[cursor] == '\'') {
                    let quote = chars[cursor];
                    cursor += 1;
                    let value_start = cursor;
                    while cursor < chars.len() && chars[cursor] != quote {
                        cursor += 1;
                    }
                    let value: String = chars[value_start..cursor].iter().collect();
                    if cursor < chars.len() {
                        cursor += 1;
                    }
                    decode_entities(&value)
                } else {
                    let value_start = cursor;
                    while cursor < chars.len()
                        && !chars[cursor].is_whitespace()
                        && chars[cursor] != '>'
                    {
                        cursor += 1;
                    }
                    decode_entities(&chars[value_start..cursor].iter().collect::<String>())
                }
            } else {
                String::new()
            };
            attributes.push((attr_name, value));
        }
        index = cursor;
        // Implied end tags.
        if !stack.is_empty() && !VOID.contains(&name.as_str()) {
            if let Some(close_name) = implies_end_of(&name).iter().find_map(|implied| {
                stack.last().is_some_and(|open| open == implied).then(|| *implied)
            }) {
                if stack.pop() == Some(close_name.to_string()) {
                    tokens.push(Token::Close(close_name.to_string()));
                }
            }
        }
        let tag = OpenTag { name: name.clone(), attributes, self_closing };
        tokens.push(Token::Open(tag.clone()));
        // Void elements close immediately (htmlparser2 emits onclosetag at
        // once).
        if self_closing || VOID.contains(&name.as_str()) {
            tokens.push(Token::Close(name.clone()));
        }
        if !self_closing && !VOID.contains(&name.as_str()) {
            stack.push(name.clone());
            if RAW_TEXT.contains(&name.as_str()) {
                // Raw text until the matching close tag.
                let mut cursor = index;
                while cursor < chars.len() {
                    if chars[cursor] == '<'
                        && chars.get(cursor + 1) == Some(&'/')
                        && chars[cursor + 2..]
                            .iter()
                            .take_while(|c| c.is_alphanumeric())
                            .collect::<String>()
                            .to_lowercase()
                            == name
                    {
                        break;
                    }
                    cursor += 1;
                }
                if cursor > index {
                    let text: String = chars[index..cursor].iter().collect();
                    if !text.is_empty() {
                        tokens.push(Token::Text(text));
                    }
                }
                index = cursor;
            }
        }
    }
    while let Some(open) = stack.pop() {
        tokens.push(Token::Close(open));
    }
    tokens
}

fn starts_with(chars: &[char], at: usize, prefix: &str) -> bool {
    let prefix: Vec<char> = prefix.chars().collect();
    if at + prefix.len() > chars.len() {
        return false;
    }
    chars[at..at + prefix.len()] == prefix[..]
}

fn find(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() {
        return Some(from);
    }
    let mut index = from;
    while index + needle.len() <= chars.len() {
        if chars[index..index + needle.len()] == needle[..] {
            return Some(index);
        }
        index += 1;
    }
    None
}

/// Entity decoding for the named entities opencode pages actually use, plus
/// all numeric forms.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '&' {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        // Find the terminator `;` within a sane window.
        let mut end = index + 1;
        while end < chars.len() && end - index <= 32 && chars[end] != ';' && chars[end] != '&' {
            end += 1;
        }
        if end >= chars.len() || chars[end] != ';' {
            out.push('&');
            index += 1;
            continue;
        }
        let body: String = chars[index + 1..end].iter().collect();
        let decoded = if let Some(hex) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X"))
        {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else if let Some(dec) = body.strip_prefix('#') {
            dec.parse::<u32>().ok().and_then(char::from_u32)
        } else {
            named_entity(&body)
        };
        match decoded {
            Some(value) => {
                out.push(value);
                index = end + 1;
            }
            None => {
                out.push('&');
                index += 1;
            }
        }
    }
    out
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "eacute" => 'é',
        "egrave" => 'è',
        "agreve" | "agrave" => 'à',
        "ccedil" => 'ç',
        "ouml" => 'ö',
        "uuml" => 'ü',
        "auml" => 'ä',
        "szlig" => 'ß',
        "laquo" => '«',
        "raquo" => '»',
        "deg" => '°',
        "plusmn" => '±',
        "times" => '×',
        "divide" => '÷',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "sect" => '§',
        "para" => '¶',
        "middot" => '·',
        "bull" => '•',
        "dagger" => '†',
        "Dagger" => '‡',
        "permil" => '‰',
        "prime" => '′',
        "Prime" => '″',
        "frasl" => '⁄',
        "larr" => '←',
        "uarr" => '↑',
        "rarr" => '→',
        "darr" => '↓',
        "harr" => '↔',
        "minus" => '−',
        "infin" => '∞',
        "ne" => '≠',
        "equiv" => '≡',
        "le" => '≤',
        "ge" => '≥',
        "alpha" => 'α',
        "beta" => 'β',
        "gamma" => 'γ',
        "delta" => 'δ',
        "pi" => 'π',
        "sigma" => 'σ',
        "omega" => 'ω',
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Converter
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct LinkData {
    href: String,
    title: Option<String>,
}

type CodeRef = Rc<RefCell<CodeData>>;

#[derive(Debug, Clone)]
struct CodeData {
    inline: bool,
    text: String,
    language: Option<String>,
}

type MarkerRef = Rc<RefCell<MarkerData>>;

#[derive(Debug, Clone)]
struct MarkerData {
    index: usize,
    block: usize,
    leading_space: bool,
    previous: Option<MarkerRef>,
}

type ListRef = Rc<RefCell<ListData>>;

#[derive(Debug, Clone)]
struct ListData {
    ordered: bool,
    next: i64,
    previous: Option<ListRef>,
}

type ItemRef = Rc<RefCell<ItemData>>;

#[derive(Debug, Clone)]
struct ItemData {
    indent: String,
    previous: Option<ItemRef>,
}

type TableRef = Rc<RefCell<TableData>>;

#[derive(Debug, Clone)]
struct TableData {
    start: usize,
    rows: Vec<Vec<String>>,
    row: Option<Vec<String>>,
    caption: Option<String>,
    fallback: bool,
    previous: Option<TableRef>,
}

type DetailsRef = Rc<RefCell<DetailsData>>;

#[derive(Debug, Clone)]
struct DetailsData {
    open: bool,
    summary: bool,
    previous: Option<DetailsRef>,
}

#[derive(Debug, Clone, Default)]
struct Frame {
    suppressed: bool,
    link: Option<Rc<LinkData>>,
    suspended_link: Option<Rc<LinkData>>,
    marker: Option<MarkerRef>,
    code: Option<CodeRef>,
    link_code: Option<CodeRef>,
    resumed_code: Option<CodeRef>,
    list: Option<ListRef>,
    item: Option<ItemRef>,
    table: Option<TableRef>,
    cell: Option<usize>,
    caption: Option<usize>,
    details: Option<DetailsRef>,
}

#[derive(Debug, Clone)]
enum Chunk {
    Text(String),
    Raw(String),
}

struct Converter {
    output: Vec<Chunk>,
    stack: Vec<Frame>,
    pending_space: bool,
    pending_indent: String,
    last: char,
    quote_depth: usize,
    needs_quote_prefix: bool,
    block_count: usize,
    depth: i64,
    stopped: bool,
    output_bytes: usize,
    active_code: Option<CodeRef>,
    active_link: Option<Rc<LinkData>>,
    link_open: bool,
    active_marker: Option<MarkerRef>,
    active_list: Option<ListRef>,
    active_item: Option<ItemRef>,
    active_table: Option<TableRef>,
    active_cell: Option<usize>,
    table_depth: usize,
    fallback_suppressed_depth: usize,
    fallback_omitted_depth: usize,
    active_details: Option<DetailsRef>,
}

fn byte_len(value: &str) -> usize {
    value.len()
}

fn slice_bytes(value: &str, bytes: usize) -> String {
    if value.len() <= bytes {
        return value.to_string();
    }
    let mut end = bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

impl Converter {
    fn new() -> Self {
        Self {
            output: Vec::new(),
            stack: Vec::new(),
            pending_space: false,
            pending_indent: String::new(),
            last: '\0',
            quote_depth: 0,
            needs_quote_prefix: false,
            block_count: 0,
            depth: 0,
            stopped: false,
            output_bytes: 0,
            active_code: None,
            active_link: None,
            link_open: false,
            active_marker: None,
            active_list: None,
            active_item: None,
            active_table: None,
            active_cell: None,
            table_depth: 0,
            fallback_suppressed_depth: 0,
            fallback_omitted_depth: 0,
            active_details: None,
        }
    }

    fn append(&mut self, value: &str, content: bool) {
        let limit = if content { CONTENT_BYTES } else { MAX_MARKDOWN_BYTES };
        if value.is_empty() || self.output_bytes >= limit {
            return;
        }
        let bytes = byte_len(value);
        let remaining = limit - self.output_bytes;
        let next =
            if bytes <= remaining { value.to_string() } else { slice_bytes(value, remaining) };
        if let Some(last) = next.chars().next_back() {
            self.last = last;
        }
        self.output_bytes += byte_len(&next);
        self.output.push(Chunk::Text(next));
    }

    fn append_raw(&mut self, value: &str) {
        let before = self.output.len();
        self.append(value, false);
        if self.output.len() > before {
            let last = self.output.len() - 1;
            self.output[last] = Chunk::Raw(match &self.output[last] {
                Chunk::Text(text) => text.clone(),
                Chunk::Raw(text) => text.clone(),
            });
        }
    }

    fn take(&mut self, start: usize) -> String {
        let chunks: Vec<Chunk> = self.output.split_off(start);
        let value: String = chunks
            .iter()
            .map(|chunk| match chunk {
                Chunk::Text(text) => text.clone(),
                Chunk::Raw(text) => text.clone(),
            })
            .collect();
        self.output_bytes = self.output_bytes.saturating_sub(byte_len(&value));
        value
    }

    fn quote_prefix(&self) -> String {
        "> ".repeat(self.quote_depth.min(8))
    }

    fn prefix_quote(&mut self) {
        if !self.needs_quote_prefix || self.quote_depth == 0 || self.active_cell.is_some() {
            return;
        }
        let prefix = self.quote_prefix();
        self.append(&prefix, false);
        self.needs_quote_prefix = false;
    }

    fn flush_space(&mut self) {
        if !self.pending_space {
            return;
        }
        let marker = self.active_marker.clone();
        if let Some(marker) = marker {
            let (index, at_edge) = {
                let marker = marker.borrow();
                (marker.index, self.output.len() == marker.index + 1)
            };
            if at_edge && self.last != ' ' && self.last != '\n' {
                if let Some(chunk) = self.output.get_mut(index) {
                    if let Chunk::Text(text) = chunk {
                        *chunk = Chunk::Text(format!(" {text}"));
                        self.output_bytes += 1;
                        marker.borrow_mut().leading_space = true;
                        self.pending_space = false;
                        return;
                    }
                }
            }
        }
        if self.last != '\0' && self.last != '\n' && self.last != ' ' {
            self.append(" ", false);
        }
        self.pending_space = false;
    }

    fn inline(&mut self, value: &str, open: bool) {
        if open {
            self.flush_space();
        }
        self.prefix_quote();
        if !self.pending_indent.is_empty() {
            let indent = std::mem::take(&mut self.pending_indent);
            self.append(&indent, false);
        }
        if self.active_link.is_some() && !self.link_open {
            self.append("[", false);
            self.link_open = true;
        }
        self.append(value, false);
    }

    fn block(&mut self) {
        if self.active_link.is_some() && self.link_open {
            let destination = self.link_destination();
            self.append(&destination, false);
            self.link_open = false;
        }
        self.pending_space = false;
        self.append("\n\n", false);
        self.block_count += 1;
        self.needs_quote_prefix = self.quote_depth > 0;
        self.pending_indent =
            self.active_item.as_ref().map_or_else(String::new, |item| item.borrow().indent.clone());
    }

    fn link_destination(&self) -> String {
        match &self.active_link {
            Some(link) => {
                format!(
                    "]({}{})",
                    escape_destination(&link.href),
                    escape_title(link.title.as_deref())
                )
            }
            None => String::new(),
        }
    }

    /// Parity surface: the TS renderer's `suspendLink(frame)` entry point
    /// (the port currently reaches the frame through `suspend_link_of_top`).
    #[allow(dead_code)]
    fn suspend_link(&mut self, frame: &mut Frame) {
        if let Some(link) = self.active_link.take() {
            frame.suspended_link = Some(link);
            self.block();
            self.link_open = false;
        }
    }

    fn text(&mut self, value: &str) {
        if let Some(code) = self.active_code.as_ref() {
            code.borrow_mut().text.push_str(value);
            return;
        }
        let mut parts: Vec<&str> = split_ws(value);
        for part in parts.drain(..) {
            if part.is_empty() {
                continue;
            }
            if part.chars().all(|c| matches!(c, '\t' | '\n' | '\u{c}' | '\r' | ' ')) {
                self.pending_space = true;
                continue;
            }
            self.flush_space();
            self.prefix_quote();
            if !self.pending_indent.is_empty() {
                let indent = std::mem::take(&mut self.pending_indent);
                self.append(&indent, false);
            }
            if self.active_link.is_some() && !self.link_open {
                self.append("[", false);
                self.link_open = true;
            }
            let escaped = escape_text(part);
            self.append(&escaped, true);
        }
    }

    fn inline_marker(name: &str) -> Option<&'static str> {
        match name {
            "strong" | "b" => Some("**"),
            "em" | "i" => Some("*"),
            "s" | "strike" | "del" => Some("~~"),
            _ => None,
        }
    }

    fn finish_code(&mut self, code: CodeRef) {
        let (inline, text) = {
            let code = code.borrow();
            (code.inline, code.text.clone())
        };
        if inline && text.is_empty() {
            return;
        }
        let mut backticks = 0;
        let mut tildes = 0;
        let mut current_backticks = 0;
        let mut current_tildes = 0;
        for character in text.chars() {
            current_backticks = if character == '`' { current_backticks + 1 } else { 0 };
            current_tildes = if character == '~' { current_tildes + 1 } else { 0 };
            backticks = backticks.max(current_backticks);
            tildes = tildes.max(current_tildes);
        }
        if inline {
            let fence = "`".repeat((backticks + 1).max(1));
            self.flush_space();
            self.prefix_quote();
            let available = CONTENT_BYTES.saturating_sub(self.output_bytes + fence.len() * 2);
            let mut payload = slice_bytes(&text, available);
            while !payload.is_empty() {
                let first = payload.chars().next().unwrap_or('\0');
                let last_char = payload.chars().next_back().unwrap_or('\0');
                let padding =
                    if (first == ' ' || first == '`' || last_char == ' ' || last_char == '`')
                        && !payload.chars().all(|c| c == ' ')
                    {
                        " "
                    } else {
                        ""
                    };
                let bytes = byte_len(&payload);
                if bytes + padding.len() * 2 <= available {
                    let value = format!("{fence}{padding}{payload}{padding}{fence}");
                    self.append_raw(&value);
                    return;
                }
                // Padding costs at most two bytes, so at most two
                // whole-code-point trims are needed.
                payload = slice_bytes(&payload, bytes.saturating_sub(1));
            }
            return;
        }
        if self.active_cell.is_some() {
            self.text(&text);
            return;
        }
        let marker = if backticks <= tildes { "`" } else { "~" };
        let length = (if marker == "`" { backticks } else { tildes } + 1).max(3);
        let fence = marker.repeat(length);
        self.block();
        let language = code.borrow().language.clone().unwrap_or_default();
        let prefix = format!("{fence}{language}\n");
        let quote = if self.quote_depth > 0 { self.quote_prefix() } else { String::new() };
        let mut payload = text;
        loop {
            let candidate = format!(
                "{prefix}{payload}{}{fence}",
                if payload.ends_with('\n') { "" } else { "\n" }
            );
            let value = if !quote.is_empty() {
                candidate
                    .lines()
                    .map(|line| format!("{quote}{line}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                candidate.clone()
            };
            let value_bytes = byte_len(&value);
            if self.output_bytes + value_bytes <= CONTENT_BYTES {
                self.append_raw(&value);
                self.block();
                return;
            }
            // No amount of trimming can make the empty fenced block fit.
            if payload.is_empty() {
                return;
            }
            let excess =
                value_bytes.saturating_sub(CONTENT_BYTES.saturating_sub(self.output_bytes));
            payload = slice_bytes(&payload, byte_len(&payload).saturating_sub(excess));
        }
    }

    fn on_open(&mut self, name: &str, attributes: &Vec<(String, String)>) {
        self.depth += 1;
        if self.depth > 10_000 {
            if self.stack.last().is_some_and(|frame| frame.suppressed) {
                let visible_parent = self.stack.iter().rposition(|frame| !frame.suppressed);
                self.fallback_suppressed_depth = visible_parent.map_or(0, |position| position + 2);
            }
            self.active_code = None;
            self.stopped = true;
        }
        if self.stopped {
            if OMITTED.contains(&name) {
                self.fallback_omitted_depth += 1;
            } else {
                self.pending_space = true;
            }
            return;
        }
        let suppressed_parent = self.stack.last().is_some_and(|frame| frame.suppressed);
        let suppressed = suppressed_parent || OMITTED.contains(&name);
        let mut frame = Frame { suppressed, ..Frame::default() };
        let hidden = attributes.iter().any(|(key, _)| key == "hidden")
            || attributes
                .iter()
                .find(|(key, _)| key == "aria-hidden")
                .is_some_and(|(_, value)| value.to_lowercase() == "true")
            || name == "head";
        let details_closed = self
            .active_details
            .as_ref()
            .is_some_and(|details| !details.borrow().open && !details.borrow().summary);
        if hidden || (details_closed && name != "summary") {
            frame.suppressed = true;
        }
        let details = self.active_details.clone();
        self.stack.push(frame);
        if self.stack.last().is_some_and(|frame| frame.suppressed) {
            return;
        }

        if let Some(code) = self.active_code.clone() {
            if !code.borrow().inline {
                if name == "br" {
                    code.borrow_mut().text.push('\n');
                }
                if name == "code" {
                    if let Some((_, class)) = attributes.iter().find(|(key, _)| key == "class") {
                        if let Some(language) = language_of(class) {
                            code.borrow_mut().language = Some(language);
                        }
                    }
                }
                return;
            }
        }
        if name == "details" {
            let open = attributes.iter().any(|(key, _)| key == "open");
            let previous = self.active_details.clone();
            let data = Rc::new(RefCell::new(DetailsData { open, summary: false, previous }));
            self.active_details = Some(Rc::clone(&data));
            if let Some(frame) = self.stack.last_mut() {
                frame.details = Some(data);
            }
            self.block();
            return;
        }
        if name == "summary" {
            if let Some(details) = &details {
                details.borrow_mut().summary = true;
            }
            self.block();
            return;
        }
        if name == "pre" {
            let code = Rc::new(RefCell::new(CodeData {
                inline: false,
                text: String::new(),
                language: None,
            }));
            self.suspend_link_of_top();
            if let Some(frame) = self.stack.last_mut() {
                frame.code = Some(Rc::clone(&code));
            }
            self.active_code = Some(code);
            return;
        }
        if name == "code" {
            if self.active_code.as_ref().is_some_and(|code| code.borrow().inline) {
                return;
            }
            let code = Rc::new(RefCell::new(CodeData {
                inline: true,
                text: String::new(),
                language: None,
            }));
            if let Some(frame) = self.stack.last_mut() {
                frame.code = Some(Rc::clone(&code));
            }
            self.active_code = Some(code);
            return;
        }
        if let Some(heading) = heading_level(name) {
            self.block();
            let value = format!("{} ", "#".repeat(heading));
            self.inline(&value, false);
            return;
        }
        if BLOCKS.contains(&name) {
            self.suspend_link_of_top();
            if name == "p" && self.last == ' ' {
                return;
            }
            self.block();
            return;
        }
        if name == "br" {
            self.pending_space = false;
            self.inline("  \n", false);
            self.needs_quote_prefix = self.quote_depth > 0;
            return;
        }
        if name == "hr" {
            self.block();
            self.inline("---", false);
            self.block();
            return;
        }
        if let Some(marker) = Self::inline_marker(name) {
            self.inline(marker, true);
            let previous = self.active_marker.clone();
            let data = Rc::new(RefCell::new(MarkerData {
                index: self.output.len().saturating_sub(1),
                block: self.block_count,
                leading_space: false,
                previous,
            }));
            self.active_marker = Some(Rc::clone(&data));
            if let Some(frame) = self.stack.last_mut() {
                frame.marker = Some(data);
            }
            return;
        }
        if name == "a" {
            let href = attr_value(attributes, "href").unwrap_or_default();
            let title = attr_value(attributes, "title");
            if self.active_link.is_some()
                && self.active_code.as_ref().is_some_and(|code| code.borrow().inline)
            {
                // finish the inline code and close the link before a nested
                // link inside it.
                let parent = self.stack.iter().rposition(|candidate| {
                    candidate.link.is_some()
                        && candidate.link_code.is_some()
                        && self.active_link.as_ref().is_some_and(|active| {
                            Rc::ptr_eq(active, candidate.link.as_ref().unwrap())
                        })
                        && self.active_code.as_ref().is_some_and(|code| {
                            Rc::ptr_eq(code, candidate.link_code.as_ref().unwrap())
                        })
                });
                if let Some(parent) = parent {
                    if let Some(code) = self.active_code.clone() {
                        self.finish_code(code);
                    }
                    if self.link_open {
                        let destination = self.link_destination();
                        self.append(&destination, false);
                    }
                    let resumed = self.stack[parent].resumed_code.clone();
                    if let Some(frame) = self.stack.get_mut(parent) {
                        frame.link = None;
                        frame.link_code = None;
                    }
                    self.active_code = resumed;
                    self.active_link = None;
                    self.link_open = false;
                }
            }
            if self.active_code.as_ref().is_some_and(|code| code.borrow().inline) {
                let resumed = self.active_code.clone();
                if let Some(frame) = self.stack.last_mut() {
                    frame.resumed_code = resumed.clone();
                }
                if let Some(code) = self.active_code.clone() {
                    if !code.borrow().text.is_empty() {
                        self.finish_code(Rc::clone(&code));
                    }
                    code.borrow_mut().text.clear();
                }
                self.active_code = None;
                let link = Rc::new(LinkData { href, title });
                if let Some(frame) = self.stack.last_mut() {
                    frame.link = Some(Rc::clone(&link));
                }
                self.active_link = Some(Rc::clone(&link));
                self.link_open = true;
                self.inline("[", true);
                let link_code = Rc::new(RefCell::new(CodeData {
                    inline: true,
                    text: String::new(),
                    language: None,
                }));
                if let Some(frame) = self.stack.last_mut() {
                    frame.link_code = Some(Rc::clone(&link_code));
                }
                self.active_code = Some(link_code);
                return;
            }
            if self.active_link.is_some() {
                if self.link_open {
                    let destination = self.link_destination();
                    self.append(&destination, false);
                }
                let active = self.active_link.clone().unwrap();
                let parent = self.stack.iter().rposition(|candidate| {
                    candidate.link.as_ref().is_some_and(|link| Rc::ptr_eq(link, &active))
                });
                if let Some(parent) = parent {
                    self.stack[parent].link = None;
                }
                self.active_link = None;
                self.link_open = false;
            }
            let link = Rc::new(LinkData { href, title });
            if let Some(frame) = self.stack.last_mut() {
                frame.link = Some(Rc::clone(&link));
            }
            self.active_link = Some(link);
            self.link_open = true;
            self.inline("[", true);
            return;
        }
        if name == "img" {
            let alt = attr_value(attributes, "alt").unwrap_or_default();
            let alt = alt.replace('\\', "\\\\").replace(']', "\\]");
            let src = attr_value(attributes, "src").unwrap_or_default();
            let title = attr_value(attributes, "title");
            let close =
                format!("]({}{})", escape_destination(&src), escape_title(title.as_deref()));
            let open = "![";
            let available =
                CONTENT_BYTES.saturating_sub(self.output_bytes + byte_len(open) + byte_len(&close));
            let alt = slice_bytes(&alt, available);
            let value = format!("{open}{alt}{close}");
            self.inline(&value, true);
            return;
        }
        if name == "blockquote" {
            self.suspend_link_of_top();
            self.block();
            self.quote_depth += 1;
            self.needs_quote_prefix = true;
            return;
        }
        if name == "ul" || name == "ol" {
            self.suspend_link_of_top();
            let start =
                attr_value(attributes, "start").and_then(|value| value.trim().parse::<i64>().ok());
            let previous = self.active_list.clone();
            let data = Rc::new(RefCell::new(ListData {
                ordered: name == "ol",
                next: start.unwrap_or(1),
                previous,
            }));
            self.active_list = Some(Rc::clone(&data));
            if let Some(frame) = self.stack.last_mut() {
                frame.list = Some(data);
            }
            self.block();
            return;
        }
        if name == "li" {
            self.block();
            let value_override =
                attr_value(attributes, "value").and_then(|value| value.trim().parse::<i64>().ok());
            if let Some(list) = &self.active_list {
                let mut list = list.borrow_mut();
                if list.ordered {
                    if let Some(value) = value_override {
                        list.next = value;
                    }
                }
            }
            let (marker, _indent_prefix) = match &self.active_list {
                Some(list) => {
                    let mut list = list.borrow_mut();
                    let marker = if list.ordered {
                        let value = list.next;
                        list.next += 1;
                        format!("{value}.")
                    } else {
                        "-".to_string()
                    };
                    (marker, list.next)
                }
                None => ("-".to_string(), 0),
            };
            let parent_indent = self
                .active_item
                .as_ref()
                .map_or_else(String::new, |item| item.borrow().indent.clone());
            let prefix = format!("{}{marker} ", parent_indent.chars().take(24).collect::<String>());
            let indent = " ".repeat(prefix.chars().count());
            let previous = self.active_item.clone();
            let data = Rc::new(RefCell::new(ItemData { indent, previous }));
            self.active_item = Some(Rc::clone(&data));
            if let Some(frame) = self.stack.last_mut() {
                frame.item = Some(data);
            }
            self.pending_indent.clear();
            let prefix = prefix.clone();
            self.inline(&prefix, false);
            return;
        }
        if name == "table" {
            self.suspend_link_of_top();
            self.table_depth += 1;
            if self.table_depth == 1 {
                self.block();
                let previous = self.active_table.clone();
                let data = Rc::new(RefCell::new(TableData {
                    start: self.output.len(),
                    rows: Vec::new(),
                    row: None,
                    caption: None,
                    fallback: false,
                    previous,
                }));
                self.active_table = Some(Rc::clone(&data));
                if let Some(frame) = self.stack.last_mut() {
                    frame.table = Some(data);
                }
            } else {
                self.pending_space = true;
            }
            return;
        }
        if name == "tr" {
            if self.table_depth != 1 {
                self.pending_space = true;
                return;
            }
            if let Some(table) = &self.active_table {
                table.borrow_mut().row = Some(Vec::new());
            }
            return;
        }
        if name == "th" || name == "td" {
            if self.table_depth != 1 {
                self.pending_space = true;
                return;
            }
            if attributes.iter().any(|(key, _)| key == "colspan" || key == "rowspan") {
                if let Some(table) = &self.active_table {
                    table.borrow_mut().fallback = true;
                }
            }
            let start = self.output.len();
            if let Some(frame) = self.stack.last_mut() {
                frame.cell = Some(start);
            }
            self.active_cell = Some(start);
            return;
        }
        if name == "caption" {
            let start = self.output.len();
            if let Some(frame) = self.stack.last_mut() {
                frame.caption = Some(start);
            }
            return;
        }
        if name == "dt" {
            self.block();
            self.inline("**", false);
            return;
        }
        if name == "dd" {
            self.inline("\n: ", false);
        }
    }

    fn suspend_link_of_top(&mut self) {
        // suspendLink(frame): block() closes the inline context (appending
        // the destination) while the link is still active, then the link
        // moves into the frame's suspended slot.
        if self.active_link.is_some() {
            self.block();
            if let Some(active) = self.active_link.take() {
                if let Some(frame) = self.stack.last_mut() {
                    frame.suspended_link = Some(active);
                }
            }
            self.link_open = false;
        }
    }

    fn on_text(&mut self, value: &str) {
        if self.stopped {
            if self.fallback_suppressed_depth == 0 && self.fallback_omitted_depth == 0 {
                self.text(value);
            }
            return;
        }
        if self.stack.last().is_some_and(|frame| frame.suppressed) {
            return;
        }
        self.text(value);
    }

    fn on_close(&mut self, name: &str) {
        self.depth -= 1;
        if self.stopped {
            if self.fallback_omitted_depth > 0 && OMITTED.contains(&name) {
                self.fallback_omitted_depth -= 1;
            }
            if self.fallback_suppressed_depth > 0
                && (self.depth as usize) < self.fallback_suppressed_depth
            {
                self.fallback_suppressed_depth = 0;
            }
            return;
        }
        let Some(frame) = self.stack.pop() else { return };
        if frame.suppressed {
            return;
        }
        if let Some(link_code) = frame.link_code.clone() {
            let frame_link = frame.link.clone();
            let matches_active = self.active_link.as_ref().is_some_and(|active| {
                frame_link.as_ref().is_some_and(|link| Rc::ptr_eq(active, link))
            });
            if !matches_active || !self.link_open {
                if let Some(resumed) = frame.resumed_code.clone() {
                    let link_text = link_code.borrow().text.clone();
                    resumed.borrow_mut().text.push_str(&link_text);
                    self.active_code = Some(resumed);
                    if self.active_link.as_ref().is_some_and(|active| {
                        frame_link.as_ref().is_some_and(|link| Rc::ptr_eq(active, link))
                    }) {
                        self.active_link = None;
                    }
                    self.link_open = false;
                    return;
                }
            }
            self.active_code = None;
            self.finish_code(link_code);
            if frame.link.is_some() && self.link_open {
                let destination = format!(
                    "]({}{})",
                    escape_destination(
                        &frame.link.as_ref().map_or(String::new(), |link| link.href.clone())
                    ),
                    escape_title(frame.link.as_ref().and_then(|link| link.title.as_deref()))
                );
                self.append(&destination, false);
            }
            self.active_link = None;
            self.link_open = false;
            self.active_code = frame.resumed_code.clone();
            return;
        }
        if self.active_code.is_some()
            && self.active_code.as_ref().is_some_and(|code| !code.borrow().inline)
            && frame.code.is_none()
        {
            return;
        }
        if let Some(code) = frame.code.clone() {
            self.active_code = None;
            self.finish_code(code);
            if let Some(suspended) = frame.suspended_link.clone() {
                self.active_link = Some(suspended);
            }
            return;
        }
        if name == "summary" {
            if let Some(details) = &self.active_details {
                details.borrow_mut().summary = false;
            }
            self.block();
            return;
        }
        if name == "details" {
            self.active_details =
                frame.details.as_ref().and_then(|details| details.borrow().previous.clone());
            self.block();
            return;
        }
        if name == "dt" {
            self.inline("**", false);
            return;
        }
        if name == "dd" {
            self.block();
            return;
        }
        if let Some(marker) = Self::inline_marker(name) {
            let trailing_space = self.pending_space;
            self.pending_space = false;
            if let Some(frame_marker) = frame.marker.clone() {
                self.active_marker = frame_marker.borrow().previous.clone();
            }
            let (stale_block, at_edge) = frame
                .marker
                .as_ref()
                .map(|frame_marker| {
                    let frame_marker = frame_marker.borrow();
                    (
                        frame_marker.block != self.block_count,
                        self.output.len() == frame_marker.index + 1,
                    )
                })
                .unwrap_or((true, false));
            if stale_block || at_edge {
                if let Some(frame_marker) = frame.marker.clone() {
                    if let Some(slot) = self.output.get_mut(frame_marker.borrow().index) {
                        *slot = Chunk::Text(String::new());
                    }
                }
                self.pending_space = trailing_space
                    || frame.marker.as_ref().is_some_and(|m| m.borrow().leading_space);
                return;
            }
            self.inline(marker, false);
            self.pending_space =
                trailing_space || frame.marker.as_ref().is_some_and(|m| m.borrow().leading_space);
            return;
        }
        if name == "a" {
            let frame_link = frame.link.clone();
            if let Some(link) = &frame_link {
                if self.active_link.as_ref().is_none_or(|active| Rc::ptr_eq(active, link)) {
                    self.active_link = Some(Rc::clone(link));
                    if self.link_open || (self.last != '\0' && self.last != '\n') {
                        let destination = format!(
                            "]({}{})",
                            escape_destination(&link.href),
                            escape_title(link.title.as_deref())
                        );
                        self.append(&destination, false);
                    }
                    self.link_open = false;
                    self.active_link = None;
                }
            }
            return;
        }
        if heading_level(name).is_some() || BLOCKS.contains(&name) {
            self.block();
            if let Some(suspended) = frame.suspended_link.clone() {
                self.active_link = Some(suspended);
            }
            return;
        }
        if name == "blockquote" {
            self.quote_depth = self.quote_depth.saturating_sub(1);
            self.block();
            if let Some(suspended) = frame.suspended_link.clone() {
                self.active_link = Some(suspended);
            }
            return;
        }
        if name == "li" {
            self.active_item = frame.item.as_ref().and_then(|item| item.borrow().previous.clone());
            self.block();
            return;
        }
        if name == "ul" || name == "ol" {
            self.active_list = frame.list.as_ref().and_then(|list| list.borrow().previous.clone());
            self.block();
            if let Some(suspended) = frame.suspended_link.clone() {
                self.active_link = Some(suspended);
            }
            return;
        }
        if (name == "th" || name == "td") && self.table_depth == 1 {
            self.active_cell = None;
            if let Some(cell_start) = frame.cell {
                let mut value = self.take(cell_start);
                value = collapse_spaces(&value);
                value = value.trim().to_string();
                value = escape_cell(&value);
                if let Some(table) = &self.active_table {
                    if let Some(row) = table.borrow_mut().row.as_mut() {
                        row.push(value);
                    }
                }
            }
            return;
        }
        if name == "tr" {
            if self.table_depth != 1 {
                return;
            }
            if let Some(table) = &self.active_table {
                let mut table = table.borrow_mut();
                if let Some(row) = table.row.take() {
                    table.rows.push(row);
                }
            }
            self.pending_space = true;
            return;
        }
        if name == "caption" && frame.caption.is_some() && self.active_table.is_some() {
            let value = collapse_spaces(&self.take(frame.caption.unwrap_or(0)));
            if let Some(table) = &self.active_table {
                table.borrow_mut().caption = Some(value.trim().to_string());
            }
            return;
        }
        if name == "table" {
            self.table_depth = self.table_depth.saturating_sub(1);
            if self.table_depth == 0 {
                let table = frame.table.clone();
                self.active_table =
                    table.as_ref().and_then(|table| table.borrow().previous.clone());
                if let Some(table) = table {
                    let (rows, fallback, caption, start) = {
                        let table = table.borrow();
                        (table.rows.clone(), table.fallback, table.caption.clone(), table.start)
                    };
                    let loose = collapse_spaces(&self.take(start));
                    let loose = loose.trim().to_string();
                    let width = rows.first().map_or(0, Vec::len);
                    let rectangular = width > 0 && rows.iter().all(|row| row.len() == width);
                    if !loose.is_empty() {
                        let loose = loose.clone();
                        self.append(&loose, false);
                        self.block();
                    }
                    if let Some(caption) = caption {
                        self.append(&caption, false);
                        self.block();
                    }
                    if !fallback && rectangular {
                        let prefix = format!(
                            "{}{}",
                            if self.quote_depth > 0 { self.quote_prefix() } else { String::new() },
                            std::mem::take(&mut self.pending_indent)
                        );
                        let header = format!(
                            "{prefix}| {} |\n{prefix}|{}",
                            rows[0].join(" | "),
                            " --- |".repeat(width)
                        );
                        self.append(&header, false);
                        for row in rows.iter().skip(1) {
                            let line = format!("\n{prefix}| {} |", row.join(" | "));
                            self.append(&line, false);
                        }
                    } else {
                        for (index, row) in rows.iter().enumerate() {
                            if index > 0 {
                                self.block();
                            }
                            let line = row.join(" | ");
                            self.append(&line, false);
                        }
                    }
                }
                self.block();
                if let Some(suspended) = frame.suspended_link.clone() {
                    self.active_link = Some(suspended);
                }
                return;
            }
            self.pending_space = true;
        }
    }

    fn finish(self) -> String {
        let mut normalized: Vec<String> = Vec::new();
        let mut pending_text = String::new();
        for chunk in &self.output {
            match chunk {
                Chunk::Raw(raw) => {
                    flush_normalized(&mut normalized, &mut pending_text);
                    normalized.push(raw.clone());
                }
                Chunk::Text(text) => pending_text.push_str(text),
            }
        }
        flush_normalized(&mut normalized, &mut pending_text);
        let joined = normalized.join("");
        let trimmed = joined.trim();
        slice_bytes(trimmed, MAX_MARKDOWN_BYTES)
    }
}

fn flush_normalized(normalized: &mut Vec<String>, pending: &mut String) {
    if pending.is_empty() {
        return;
    }
    // /[ \t]+\n/ → "  \n" keeps two-or-more-space hard breaks.
    let text = pending.replace("\r\n", "\n");
    let mut squashed = String::with_capacity(text.len());
    let mut index = 0;
    let chars: Vec<char> = text.chars().collect();
    while index < chars.len() {
        let run_start = index;
        while index < chars.len() && (chars[index] == ' ' || chars[index] == '\t') {
            index += 1;
        }
        if index < chars.len() && chars[index] == '\n' && index > run_start {
            let run: String = chars[run_start..index].iter().collect();
            if run.starts_with("  ") {
                squashed.push_str("  \n");
            } else {
                squashed.push('\n');
            }
            index += 1;
            continue;
        }
        if index == run_start {
            squashed.push(chars[index]);
            index += 1;
        } else {
            squashed.push_str(&chars[run_start..index].iter().collect::<String>());
        }
    }
    // /\n{3,}/ → "\n\n"
    while squashed.contains("\n\n\n") {
        squashed = squashed.replace("\n\n\n", "\n\n");
    }
    let lines: Vec<&str> = squashed.split('\n').collect();
    let mapped: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if !line.is_empty() {
                return (*line).to_string();
            }
            let previous = lines[index.saturating_sub(1)];
            let before = previous.starts_with(">").then(|| quote_prefix_of(previous));
            let after = lines.get(index + 1).map(|next| quote_prefix_of(next));
            match (before, after) {
                (Some(before), Some(after)) if before != after => {
                    let shorter = before.min(after);
                    let prefix = "> ".repeat(shorter / 2);
                    prefix.trim_end().to_string()
                }
                _ => String::new(),
            }
        })
        .collect();
    normalized.push(mapped.join("\n"));
    pending.clear();
}

fn quote_prefix_of(line: &str) -> usize {
    let mut count = 0;
    let mut rest = line;
    while let Some(tail) = rest.strip_prefix("> ") {
        count += 2;
        rest = tail;
    }
    count
}

fn split_ws(value: &str) -> Vec<&str> {
    // split(/([\t\n\f\r ]+)/) — alternating non-ws / ws segments.
    let is_ws = |c: char| matches!(c, '\t' | '\n' | '\u{c}' | '\r' | ' ');
    let mut parts = Vec::new();
    let mut start = 0;
    let mut current: Option<bool> = None;
    for (position, char) in value.char_indices() {
        let ws = is_ws(char);
        match current {
            None => current = Some(ws),
            Some(previous) if previous != ws => {
                parts.push(&value[start..position]);
                start = position;
                current = Some(ws);
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts
}

fn escape_text(part: &str) -> String {
    let mut escaped = String::with_capacity(part.len());
    for char in part.chars() {
        match char {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' => {
                escaped.push('\\');
                escaped.push(char);
            }
            '~' => escaped.push_str("\\~"),
            _ => escaped.push(char),
        }
    }
    // Leading markers: ^([#+-]) and ^(\d+)\.
    if let Some(first) = escaped.chars().next() {
        if ['#', '+', '-'].contains(&first) {
            escaped = format!("\\{escaped}");
        }
    }
    if let Some(position) = escaped.find('.') {
        if position > 0 && escaped[..position].chars().all(|c| c.is_ascii_digit()) {
            let mut rewritten = escaped[..position].to_string();
            rewritten.push_str("\\.");
            rewritten.push_str(&escaped[position + 1..]);
            escaped = rewritten;
        }
    }
    escaped
}

fn escape_destination(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for char in value.chars() {
        match char {
            '\\' | '(' | ')' => {
                escaped.push('\\');
                escaped.push(char);
            }
            '\t' | '\n' | '\r' | ' ' => escaped.push_str("%20"),
            _ => escaped.push(char),
        }
    }
    escaped
}

fn escape_title(value: Option<&str>) -> String {
    let Some(value) = value else { return String::new() };
    let collapsed = collapse_spaces(value);
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut escaped = String::with_capacity(trimmed.len());
    for char in trimmed.chars() {
        match char {
            '\\' | '"' => {
                escaped.push('\\');
                escaped.push(char);
            }
            _ => escaped.push(char),
        }
    }
    format!(" \"{escaped}\"")
}

fn escape_cell(value: &str) -> String {
    // Replace unescaped pipes.
    let mut escaped = String::with_capacity(value.len());
    let mut previous_backslash = false;
    for char in value.chars() {
        if char == '|' && !previous_backslash {
            escaped.push_str("\\|");
            previous_backslash = false;
            continue;
        }
        previous_backslash = char == '\\';
        escaped.push(char);
    }
    escaped
}

fn collapse_spaces(value: &str) -> String {
    let mut collapsed = String::with_capacity(value.len());
    let mut in_run = false;
    for char in value.chars() {
        if matches!(char, '\t' | '\r' | '\n' | ' ') {
            if !in_run {
                collapsed.push(' ');
                in_run = true;
            }
        } else {
            collapsed.push(char);
            in_run = false;
        }
    }
    collapsed
}

fn heading_level(name: &str) -> Option<usize> {
    let bytes = name.as_bytes();
    if bytes.len() == 2 && bytes[0] == b'h' && (b'1'..=b'6').contains(&bytes[1]) {
        Some((bytes[1] - b'0') as usize)
    } else {
        None
    }
}

fn attr_value(attributes: &[(String, String)], name: &str) -> Option<String> {
    attributes.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone())
}

fn language_of(class: &str) -> Option<String> {
    // /(?:language-|lang-)([^\s]+)/
    let position = class
        .find("language-")
        .map(|p| p + "language-".len())
        .or_else(|| class.find("lang-").map(|p| p + "lang-".len()))?;
    let language: String = class[position..].chars().take_while(|c| !c.is_whitespace()).collect();
    (!language.is_empty()).then_some(language)
}

/// `MAX_MARKDOWN_BYTES` re-export (webfetch budget).
pub const MAX_BYTES: usize = MAX_MARKDOWN_BYTES;

/// `convertHTMLToMarkdown`.
#[must_use]
pub fn convert_html_to_markdown(html: &str) -> String {
    let mut converter = Converter::new();
    for token in tokenize(html) {
        match token {
            Token::Open(tag) => converter.on_open(&tag.name, &tag.attributes),
            Token::Close(name) => converter.on_close(&name),
            Token::Text(text) => converter.on_text(&text),
        }
    }
    converter.finish()
}

/// `WebFetchTool.extractTextFromHTML` — text format skipping script/style/
/// noscript/iframe/object/embed.
#[must_use]
pub fn extract_text_from_html(html: &str) -> String {
    let mut text = String::new();
    let mut skip_depth = 0;
    for token in tokenize(html) {
        match token {
            Token::Open(tag) => {
                if skip_depth > 0
                    || ["script", "style", "noscript", "iframe", "object", "embed"]
                        .contains(&tag.name.as_str())
                {
                    skip_depth += 1;
                }
            }
            Token::Close(_) => {
                if skip_depth > 0 {
                    skip_depth -= 1;
                }
            }
            Token::Text(value) => {
                if skip_depth == 0 {
                    text.push_str(&value);
                }
            }
        }
    }
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_html_markdown_text_and_basic_conversions() {
        let html = "<h1>Hello</h1><script>bad()</script><p>world <strong>wide</strong> <product-name>today</product-name></p><style>.bad {}</style>";
        assert_eq!(extract_text_from_html(html), "Helloworld wide today");
        assert_eq!(convert_html_to_markdown(html), "# Hello\n\nworld **wide** today");
    }

    #[test]
    fn tool_html_markdown_headings_links_images_breaks() {
        let html = "<h2>Read <em>this</em></h2><p><a href=\"https://example.com/a (b)\" title=\"Example\">docs</a><br><img src=\"diagram.png\" alt=\"a ] b\"></p><hr><p><del>old</del></p>";
        assert_eq!(
            convert_html_to_markdown(html),
            "## Read *this*\n\n[docs](https://example.com/a%20\\(b\\) \"Example\")  \n![a \\] b](diagram.png)\n\n---\n\n~~old~~"
        );
    }

    #[test]
    fn tool_html_markdown_preformatted_code_with_safe_fences() {
        let html = "<p>Use <code>say(`hello`)</code> now.</p><pre><code class=\"language-ts\">const fence = ```\n&amp; stays decoded</code></pre>";
        assert_eq!(
            convert_html_to_markdown(html),
            "Use ``say(`hello`)`` now.\n\n~~~ts\nconst fence = ```\n& stays decoded\n~~~"
        );
    }

    #[test]
    fn tool_html_markdown_inline_code_boundaries() {
        let cases: &[(&str, &str)] = &[
            ("`x`", "`` `x` ``"),
            ("`x", "`` `x ``"),
            ("x`", "`` x` ``"),
            ("`", "`` ` ``"),
            ("``", "``` `` ```"),
            ("``x`", "``` ``x` ```"),
            ("say(`x`)", "``say(`x`)``"),
            ("a``b`c", "```a``b`c```"),
            ("x", "`x`"),
            (" x ", "`  x  `"),
            (" x", "`  x `"),
            ("x ", "` x  `"),
            ("   ", "`   `"),
            (" ` ", "``  `  ``"),
        ];
        for (content, expected) in cases {
            let html = format!("<p>Use <code>{content}</code>.</p>");
            assert_eq!(
                convert_html_to_markdown(&html),
                format!("Use {expected}."),
                "content {content:?}"
            );
        }
    }

    #[test]
    fn tool_html_markdown_nested_lists() {
        let html = "<ol start=\"3\"><li>alpha<ul><li>nested <strong>item</strong></li></ul></li><li><p>beta first</p><p>beta second</p></li></ol>";
        assert_eq!(
            convert_html_to_markdown(html),
            "3. alpha\n\n   - nested **item**\n\n4. beta first\n\n   beta second"
        );
    }

    #[test]
    fn tool_html_markdown_blockquotes_and_tables() {
        let html = "<blockquote><p>quoted <em>text</em></p><ul><li>point</li></ul></blockquote><table><thead><tr><th>Name</th><th>Value</th></tr></thead><tbody><tr><td>one</td><td><code>1</code></td></tr></tbody></table>";
        assert_eq!(
            convert_html_to_markdown(html),
            "> quoted *text*\n\n> - point\n\n| Name | Value |\n| --- | --- |\n| one | `1` |"
        );
    }

    #[test]
    fn tool_html_markdown_decodes_entities() {
        let html = "<p>alpha\n  <span>&amp; beta</span> <unknown>caf&eacute;</unknown>&nbsp;gamma 😀</p><p>delta</p>";
        assert_eq!(convert_html_to_markdown(html), "alpha & beta café\u{a0}gamma 😀\n\ndelta");
    }

    #[test]
    fn tool_html_markdown_omits_active_content() {
        let html = "<p>before <script><b>bad</b></script><style>bad</style><noscript>bad</noscript><iframe>bad</iframe><object>bad</object><embed src=\"bad\"><meta content=\"bad\"><link href=\"bad\"><template>bad</template> after</p>";
        assert_eq!(convert_html_to_markdown(html), "before after");
    }

    #[test]
    fn tool_html_markdown_escapes_prose_structure() {
        assert_eq!(
            convert_html_to_markdown("<p># heading</p><p>1. item</p><p>---</p><p>a | b</p>"),
            "\\# heading\n\n1\\. item\n\n\\---\n\na \\| b"
        );
    }

    #[test]
    fn tool_html_markdown_preserves_code_whitespace_in_quotes() {
        let html = "<blockquote><pre>line  \n\n\nnext</pre><table><tr><td>a|b</td><td>c</td></tr></table></blockquote>";
        assert_eq!(
            convert_html_to_markdown(html),
            "> ```\n> line  \n> \n> \n> next\n> ```\n\n> | a\\|b | c |\n> | --- | --- |"
        );
    }

    #[test]
    fn tool_html_markdown_nested_blockquotes() {
        let html =
            "<blockquote><p>outer</p><blockquote><p>inner</p></blockquote><p>end</p></blockquote>";
        assert_eq!(convert_html_to_markdown(html), "> outer\n>\n> > inner\n>\n> end");
    }

    #[test]
    fn tool_html_markdown_visible_whitespace_around_emphasis() {
        assert_eq!(
            convert_html_to_markdown("<p>a<strong> b</strong> c a <em>b </em>c</p>"),
            "a **b** c a *b* c"
        );
        assert_eq!(convert_html_to_markdown("a<strong> </strong>b a<em> </em>b"), "a b a b");
    }

    #[test]
    fn tool_html_markdown_preformatted_formatting_is_code_only() {
        assert_eq!(
            convert_html_to_markdown("<pre><b>x</b><i>y</i><del>z</del></pre>"),
            "```\nxyz\n```"
        );
    }

    #[test]
    fn tool_html_markdown_table_cells_multiline() {
        let html = "<table><tr><td>x<br>y</td><td><code>a|b</code></td><td><p>first</p><p>second</p></td></tr></table>";
        assert_eq!(
            convert_html_to_markdown(html),
            "| x y | `a\\|b` | first second |\n| --- | --- | --- |"
        );
    }

    #[test]
    fn tool_html_markdown_nested_tables_flatten() {
        let html = "<table><tr><th>Parent</th><th>Sibling</th></tr><tr><td>Before<table><tr><th>Key</th><th>Value</th></tr><tr><td>A</td><td>1</td></tr></table>After</td><td>Tail</td></tr></table>";
        assert_eq!(
            convert_html_to_markdown(html),
            "| Parent | Sibling |\n| --- | --- |\n| Before Key Value A 1 After | Tail |"
        );
    }

    #[test]
    fn tool_html_markdown_loose_table_text() {
        assert_eq!(
            convert_html_to_markdown("<table>before<tr><td>cell</td></tr>after</table>"),
            "before after\n\n| cell |\n| --- |"
        );
        assert_eq!(convert_html_to_markdown("<table>alpha</table>"), "alpha");
    }

    #[test]
    fn tool_html_markdown_tilde_escapes_and_empty_emphasis() {
        assert_eq!(
            convert_html_to_markdown("<p>~~~</p><p><strong></strong>content</p><p>~~~</p>"),
            "\\~\\~\\~\n\ncontent\n\n\\~\\~\\~"
        );
    }

    #[test]
    fn tool_html_markdown_multiline_inline_code() {
        assert_eq!(
            convert_html_to_markdown("<p><code>first\n\n\nsecond  </code></p>"),
            "` first\n\n\nsecond   `"
        );
    }

    #[test]
    fn tool_html_markdown_blockquote_inline_code() {
        assert_eq!(
            convert_html_to_markdown("<blockquote><code>x</code> y</blockquote>"),
            "> `x` y"
        );
    }

    #[test]
    fn tool_html_markdown_lists_indent_and_numbering() {
        let html = "<ol start=\"0\"><li value=\"4\"><p>first</p><p>continued</p><ul><li><p>nested</p><p>continued nested</p></li></ul></li><li>next</li></ol>";
        assert_eq!(
            convert_html_to_markdown(html),
            "4. first\n\n   continued\n\n   - nested\n\n     continued nested\n\n5. next"
        );
    }

    #[test]
    fn tool_html_markdown_block_content_outside_link_syntax() {
        assert_eq!(
            convert_html_to_markdown("<a href=\"/docs\">before<div>block</div>after</a>"),
            "[before](/docs)\n\nblock\n\n[after](/docs)"
        );
    }

    #[test]
    fn tool_html_markdown_is_bounded() {
        let output = convert_html_to_markdown(&"x".repeat(MAX_MARKDOWN_BYTES));
        assert_eq!(MAX_MARKDOWN_BYTES, 5 * 1024 * 1024);
        assert_eq!(byte_len(&output), MAX_MARKDOWN_BYTES - 64 * 1024);
    }

    #[test]
    fn tool_html_markdown_fits_content_budget_exactly() {
        let budget = MAX_MARKDOWN_BYTES - 64 * 1024;
        let fitting = format!("aa{}", "é".repeat(budget / 2 - 1));
        assert_eq!(convert_html_to_markdown(&fitting), fitting);
        let truncated = convert_html_to_markdown(&format!("{fitting}é"));
        assert_eq!(truncated, fitting);
    }
}
