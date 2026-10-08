//! Incremental JSON parser (port of `src/protocols/utils/partial-json.ts`,
//! itself adapted from Promplate's partial-json-parser — MIT).
//!
//! Parses complete **or incomplete** JSON restricted by partial-value flags
//! (port of `partial-json.test.ts`). One divergence from the TS original:
//! `Infinity`/`NaN` literals cannot become `serde_json::Value`s, so those
//! two `Allow` flags parse to an error instead — tool-call argument streams
//! never legitimately contain them.

/// Allow-partial bit flags (port of `partial-json-options.ts`).
pub mod allow {
    pub const STR: u32 = 0b0000_0001;
    pub const NUM: u32 = 0b0000_0010;
    pub const ARR: u32 = 0b0000_0100;
    pub const OBJ: u32 = 0b0000_1000;
    pub const NULL: u32 = 0b0001_0000;
    pub const BOOL: u32 = 0b0010_0000;
    pub const NAN: u32 = 0b0100_0000;
    pub const INFINITY: u32 = 0b1000_0000;
    pub const _INFINITY: u32 = 0b1_0000_0000;
    pub const INF: u32 = INFINITY | _INFINITY;
    pub const SPECIAL: u32 = NULL | BOOL | INF | NAN;
    pub const ATOM: u32 = STR | NUM | SPECIAL;
    pub const COLLECTION: u32 = ARR | OBJ;
    pub const ALL: u32 = ATOM | COLLECTION;
}

pub use allow::ALL as Allow;

/// The input ended before a complete value of an allowed type could form.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("PartialJSON: {0}")]
pub struct PartialJSONError(pub String);

/// The input cannot be interpreted as (partial) JSON under these flags.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("MalformedJSON: {0}")]
pub struct MalformedJSONError(pub String);

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    Partial(PartialJSONError),
    Malformed(MalformedJSONError),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Partial(error) => write!(f, "{error}"),
            ParseError::Malformed(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse complete or incomplete JSON, restricted by the supplied
/// partial-value flags (port of `parseJSON`).
pub fn parse(json_string: &str, allow_partial: u32) -> Result<serde_json::Value, ParseError> {
    let input = json_string.trim();
    if input.is_empty() {
        return Err(ParseError::Malformed(MalformedJSONError(format!("{json_string} is empty"))));
    }
    if let Ok(value) = serde_json::from_str(input) {
        return Ok(value);
    }
    let repaired = repair_json(input);
    if repaired != input {
        if let Ok(value) = serde_json::from_str(&repaired) {
            return Ok(value);
        }
    }
    match parse_incremental(input, allow_partial) {
        Ok(value) => Ok(value),
        Err(error) => {
            if repaired == input {
                Err(error)
            } else {
                parse_incremental(&repaired, allow_partial)
            }
        }
    }
}

/// Repair invalid escapes and raw control characters inside string literals
/// (port of `repairJSON`).
fn repair_json(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut repaired = String::with_capacity(input.len());
    let mut quoted = false;
    let mut index = 0;
    while index < bytes.len() {
        let character = bytes[index];
        if !quoted {
            let ch_len = if character >= 0x80 { utf8_len(character) } else { 1 };
            repaired.push_str(&input[index..(index + ch_len).min(input.len())]);
            if character == b'"' {
                quoted = true;
            }
            index += ch_len;
            continue;
        }
        if character == b'"' {
            repaired.push('"');
            quoted = false;
            index += 1;
            continue;
        }
        if character == b'\\' {
            let next = bytes.get(index + 1).copied();
            if next == Some(b'u')
                && index + 6 <= input.len()
                && input[index + 2..index + 6].bytes().all(|b| b.is_ascii_hexdigit())
            {
                repaired.push_str(&input[index..index + 6]);
                index += 6;
                continue;
            }
            if let Some(next) = next {
                if b"\"\\/bfnrtu".contains(&next) {
                    repaired.push('\\');
                    repaired.push(next as char);
                    index += 2;
                    continue;
                }
            }
            repaired.push_str("\\\\");
            index += 1;
            continue;
        }
        if character <= 0x1f {
            repaired.push_str(&format!("\\u{character:04x}"));
        } else {
            // printable ASCII or UTF-8 content: copy the full sequence
            let ch_len = utf8_len(character);
            repaired.push_str(&input[index..(index + ch_len).min(input.len())]);
            index += ch_len;
            continue;
        }
        index += 1;
    }
    repaired
}

fn utf8_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

struct Scanner<'a> {
    input: &'a str,
    bytes: &'a [u8],
    index: usize,
}

impl Scanner<'_> {
    fn partial(&self, message: &str) -> ParseError {
        ParseError::Partial(PartialJSONError(format!("{message} at position {}", self.index)))
    }

    fn malformed(&self, message: &str) -> ParseError {
        ParseError::Malformed(MalformedJSONError(format!("{message} at position {}", self.index)))
    }

    fn current(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn starts_with(&self, literal: &str) -> bool {
        self.input[self.index..].starts_with(literal)
    }

    fn skip_blank(&mut self) {
        while let Some(byte) = self.current() {
            if matches!(byte, b' ' | b'\n' | b'\r' | b'\t') {
                self.index += 1;
            } else {
                break;
            }
        }
    }

    fn parse_any(&mut self, allow_flags: u32) -> Result<serde_json::Value, ParseError> {
        self.skip_blank();
        if self.index >= self.bytes.len() {
            return Err(self.partial("Unexpected end of input"));
        }
        match self.current() {
            Some(b'"') => self.parse_string(allow_flags),
            Some(b'{') => self.parse_object(allow_flags),
            Some(b'[') => self.parse_array(allow_flags),
            _ => {
                // literal keywords with partial prefixes
                let remaining_len = self.bytes.len() - self.index;
                let remaining = &self.input[self.index..];
                if self.starts_with("null")
                    || ((allow_flags & allow::NULL != 0)
                        && remaining_len < 4
                        && "null".starts_with(remaining))
                {
                    self.index += 4;
                    return Ok(serde_json::Value::Null);
                }
                if self.starts_with("true")
                    || ((allow_flags & allow::BOOL != 0)
                        && remaining_len < 4
                        && "true".starts_with(remaining))
                {
                    self.index += 4;
                    return Ok(serde_json::Value::Bool(true));
                }
                if self.starts_with("false")
                    || ((allow_flags & allow::BOOL != 0)
                        && remaining_len < 5
                        && "false".starts_with(remaining))
                {
                    self.index += 5;
                    return Ok(serde_json::Value::Bool(false));
                }
                if self.starts_with("Infinity")
                    || ((allow_flags & allow::INFINITY != 0)
                        && remaining_len < 8
                        && "Infinity".starts_with(remaining))
                {
                    return Err(self.malformed("Infinity is not representable in JSON"));
                }
                if self.starts_with("-Infinity")
                    || ((allow_flags & allow::_INFINITY != 0)
                        && 1 < remaining_len
                        && remaining_len < 9
                        && "-Infinity".starts_with(remaining))
                {
                    return Err(self.malformed("-Infinity is not representable in JSON"));
                }
                if self.starts_with("NaN")
                    || ((allow_flags & allow::NAN != 0)
                        && remaining_len < 3
                        && "NaN".starts_with(remaining))
                {
                    return Err(self.malformed("NaN is not representable in JSON"));
                }
                self.parse_number(allow_flags)
            }
        }
    }

    fn parse_string(&mut self, allow_flags: u32) -> Result<serde_json::Value, ParseError> {
        let start = self.index;
        let mut escape = false;
        self.index += 1;
        while self.index < self.bytes.len() {
            let byte = self.bytes[self.index];
            if byte == b'"' && !(escape && self.bytes[self.index - 1] == b'\\') {
                break;
            }
            escape = if byte == b'\\' { !escape } else { false };
            if byte >= 0x80 {
                self.index += utf8_len(byte);
            } else {
                self.index += 1;
            }
        }
        if self.current() == Some(b'"') {
            self.index += 1;
            let end = self.index - usize::from(escape);
            let text = &self.input[start..end];
            return serde_json::from_str(text).map_err(|error| self.malformed(&error.to_string()));
        }
        if allow_flags & allow::STR != 0 {
            let end = (self.index - usize::from(escape)).max(start);
            let text = &self.input[start..end];
            if let Ok(value) = serde_json::from_str(&format!("{text}\"")) {
                return Ok(value);
            }
            if let Some(last_backslash) = text.rfind('\\') {
                let truncated = &text[..last_backslash];
                if let Ok(value) = serde_json::from_str(&format!("{truncated}\"")) {
                    return Ok(value);
                }
            }
        }
        Err(self.partial("Unterminated string literal"))
    }

    fn parse_object(&mut self, allow_flags: u32) -> Result<serde_json::Value, ParseError> {
        self.index += 1;
        let mut object = serde_json::Map::new();
        let result = self.parse_object_body(&mut object, allow_flags);
        match result {
            Ok(()) => {
                self.index += 1; // closing brace
                Ok(serde_json::Value::Object(object))
            }
            Err(error) => {
                if allow_flags & allow::OBJ != 0 {
                    Ok(serde_json::Value::Object(object))
                } else {
                    Err(self.partial(&format!("Expected '}}' at end of object: {error}")))
                }
            }
        }
    }

    fn parse_object_body(
        &mut self,
        object: &mut serde_json::Map<String, serde_json::Value>,
        allow_flags: u32,
    ) -> Result<(), ParseError> {
        self.skip_blank();
        while self.current() != Some(b'}') {
            self.skip_blank();
            if self.index >= self.bytes.len() {
                return Err(self.partial("Unexpected end of input"));
            }
            let key = match self.parse_string(allow_flags) {
                Ok(serde_json::Value::String(key)) => key,
                Ok(_) => return Err(self.malformed("object keys must be strings")),
                Err(error) => return Err(error),
            };
            self.skip_blank();
            self.index += 1; // colon
            let value = self.parse_any(allow_flags)?;
            object.insert(key, value);
            self.skip_blank();
            if self.current() == Some(b',') {
                self.index += 1;
            }
        }
        Ok(())
    }

    fn parse_array(&mut self, allow_flags: u32) -> Result<serde_json::Value, ParseError> {
        self.index += 1;
        let mut array = Vec::new();
        let result = self.parse_array_body(&mut array, allow_flags);
        match result {
            Ok(()) => {
                self.index += 1; // closing bracket
                Ok(serde_json::Value::Array(array))
            }
            Err(error) => {
                if allow_flags & allow::ARR != 0 {
                    Ok(serde_json::Value::Array(array))
                } else {
                    Err(self.partial(&format!("Expected ']' at end of array: {error}")))
                }
            }
        }
    }

    fn parse_array_body(
        &mut self,
        array: &mut Vec<serde_json::Value>,
        allow_flags: u32,
    ) -> Result<(), ParseError> {
        while self.current() != Some(b']') {
            array.push(self.parse_any(allow_flags)?);
            self.skip_blank();
            if self.current() == Some(b',') {
                self.index += 1;
            }
        }
        Ok(())
    }

    fn parse_number(&mut self, allow_flags: u32) -> Result<serde_json::Value, ParseError> {
        if self.index == 0 {
            if self.input == "-" {
                return Err(self.malformed("Not sure what '-' is"));
            }
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(self.input) {
                return Ok(value);
            }
            if allow_flags & allow::NUM != 0 {
                if let Some(last_e) = self.input.rfind(['e', 'E']) {
                    if let Ok(value) =
                        serde_json::from_str::<serde_json::Value>(&self.input[..last_e])
                    {
                        return Ok(value);
                    }
                }
            }
            return Err(self.malformed("Invalid number literal"));
        }
        let start = self.index;
        if self.current() == Some(b'-') {
            self.index += 1;
        }
        while let Some(byte) = self.current() {
            if byte == b',' || byte == b']' || byte == b'}' {
                break;
            }
            self.index += if byte >= 0x80 { utf8_len(byte) } else { 1 };
        }
        if self.index >= self.bytes.len() && allow_flags & allow::NUM == 0 {
            return Err(self.partial("Unterminated number literal"));
        }
        let text = &self.input[start..self.index.min(self.input.len())];
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
            return Ok(value);
        }
        if text == "-" {
            return Err(self.partial("Not sure what '-' is"));
        }
        if let Some(last_e) = text.rfind(['e', 'E']) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text[..last_e]) {
                return Ok(value);
            }
        }
        Err(self.malformed("Invalid number literal"))
    }
}

fn parse_incremental(input: &str, allow_flags: u32) -> Result<serde_json::Value, ParseError> {
    Scanner { input, bytes: input.as_bytes(), index: 0 }.parse_any(allow_flags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_complete_json() {
        assert_eq!(
            parse(r#"{"key":"value","items":[1,true,null]}"#, allow::ALL).unwrap(),
            json!({"key": "value", "items": [1, true, null]})
        );
        // "__proto__" is an ordinary key in Rust maps
        let object = parse(r#"{"__proto__":{"safe":true}}"#, allow::ALL).unwrap();
        assert_eq!(object.pointer("/__proto__/safe"), Some(&json!(true)));
    }

    #[test]
    fn parses_partial_strings() {
        assert_eq!(parse(r#""hello"#, allow::ALL).unwrap(), json!("hello"));
        assert_eq!(parse(r#""hello \u12"#, allow::ALL).unwrap(), json!("hello "));
        assert!(parse(r#""hello"#, allow::ALL & !allow::STR).is_err());
    }

    #[test]
    fn repairs_invalid_escapes_and_raw_control_characters() {
        assert_eq!(
            parse(r#"{"path":"A\H","text":"first	second"}"#, allow::ALL).unwrap(),
            json!({"path": "A\\H", "text": "first\tsecond"})
        );
    }

    #[test]
    fn preserves_prototype_keys_in_partial_objects() {
        let object = parse(r#"{"__proto__":{"safe":true}"#, allow::ALL).unwrap();
        assert_eq!(object.pointer("/__proto__/safe"), Some(&json!(true)));
    }

    #[test]
    fn controls_partial_collection_values_independently() {
        assert_eq!(parse(r#"[""#, allow::ARR).unwrap(), json!([]));
        assert_eq!(parse(r#"[""#, allow::ARR | allow::STR).unwrap(), json!([""]));
        assert_eq!(parse(r#"{"key":""#, allow::OBJ).unwrap(), json!({}));
        assert_eq!(parse(r#"{"key":""#, allow::OBJ | allow::STR).unwrap(), json!({"key": ""}));
    }

    #[test]
    fn parses_partial_literals_and_numbers() {
        assert_eq!(parse("nu", allow::NULL).unwrap(), json!(null));
        assert_eq!(parse("tr", allow::BOOL).unwrap(), json!(true));
        assert_eq!(parse("fa", allow::BOOL).unwrap(), json!(false));
        assert_eq!(parse("1e", allow::NUM).unwrap(), json!(1));
    }

    #[test]
    fn distinguishes_disallowed_partial_values_from_malformed_values() {
        assert!(matches!(parse("[", allow::STR).unwrap_err(), ParseError::Partial(_)));
        assert!(matches!(
            parse("n", allow::ALL & !allow::NULL).unwrap_err(),
            ParseError::Malformed(_)
        ));
    }

    #[test]
    fn rejects_empty_input() {
        assert!(parse("  ", allow::ALL).unwrap_err().to_string().contains("is empty"));
    }

    #[test]
    fn incremental_object_accumulation_matches_streamed_tool_input() {
        // OpenAI-style streamed tool arguments, one delta at a time
        let deltas = [r#"{"query""#, r":", r#""weather""#, r"}"];
        let mut accumulated = String::new();
        let mut last = serde_json::Value::Null;
        for delta in deltas {
            accumulated.push_str(delta);
            last = parse(&accumulated, allow::ALL).unwrap_or(last.clone());
        }
        assert_eq!(last, json!({"query": "weather"}));
    }
}
