//! G-281① — strict JSONC parsing (ref `config.ts:106`
//! `parse(text, errors, { allowTrailingComma: true })`): v2's configs
//! accept comments and trailing commas and NOTHING else — every other
//! deviation from strict JSON fills jsonc-parser's `errors` array and
//! REJECTS the document. The port previously used json5, silently
//! accepting the wider ECMA-264 superset (single-quoted strings,
//! unquoted keys, hex numbers, `Infinity`, …) as valid configuration.
//!
//! [`parse_jsonc`] strips the two legal extensions (comments, trailing
//! commas) with a string-aware scanner, then hands the result to the
//! strict `serde_json` parser — anything beyond JSONC fails exactly
//! like ref's `errors.length` branch.

use serde_json::Value;

use super::Json;

/// The unit error of [`parse_jsonc`]: any laxness beyond comments and
/// trailing commas is rejected (mirrors ref's error branch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsoncError;

/// Parse a JSONC document: comments and trailing commas allowed, any
/// further laxness rejected ([`JsoncError`] mirrors ref's error branch).
pub fn parse_jsonc(text: &str) -> Result<Value, JsoncError> {
    let cleaned = strip_jsonc(text);
    serde_json::from_str(&cleaned).map_err(|_| JsoncError)
}

/// G-413 — parse straight into the insertion-ordered [`Json`] shape
/// via serde's streaming deserializer (map keys are visited in
/// input-text order). Routing through `serde_json::Value` first would
/// materialize each object as a BTreeMap (decision R-03:
/// `preserve_order` stays off) and destroy the source key order the v1
/// permission migration needs (`normalize.ts:515-519` iterates
/// `Object.entries`). Same strictness as [`parse_jsonc`].
pub fn parse_jsonc_ordered(text: &str) -> Result<Json, JsoncError> {
    let cleaned = strip_jsonc(text);
    serde_json::from_str::<Json>(&cleaned).map_err(|_| JsoncError)
}

/// Removes `//…` and `/*…*/` comments, trailing commas, a leading BOM,
/// and escapes raw control characters inside strings (jsonc-parser
/// tolerates those silently; `serde_json` would reject them — escaping
/// keeps the accepted set identical). String-aware: `//` or `,` inside
/// a string literal is left untouched, `\"` escapes honored.
#[must_use]
pub fn strip_jsonc(text: &str) -> String {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let bytes: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    // Byte position (into `out`) of the last pending comma — cleared on
    // any other significant char, popped when the enclosing bracket
    // closes (making it a trailing comma).
    let mut pending_comma: Option<usize> = None;
    while index < bytes.len() {
        let char = bytes[index];
        if in_string {
            out.push(char);
            if escaped {
                escaped = false;
            } else if char == '\\' {
                escaped = true;
            } else if char == '"' {
                in_string = false;
                pending_comma = None;
            } else if (char as u32) < 0x20 {
                // Raw control char inside a string: re-escape so the
                // strict pass accepts it like jsonc-parser does.
                let encoded = format!("\\u{:04x}", char as u32);
                out.pop();
                out.push_str(&encoded);
            }
            index += 1;
            continue;
        }
        match char {
            '"' => {
                out.push('"');
                in_string = true;
                escaped = false;
                pending_comma = None;
            }
            '/' if bytes.get(index + 1) == Some(&'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != '\n' {
                    index += 1;
                }
                continue;
            }
            '/' if bytes.get(index + 1) == Some(&'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == '*' && bytes[index + 1] == '/') {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
                continue;
            }
            ',' => {
                out.push(',');
                pending_comma = Some(out.len() - 1);
            }
            '}' | ']' => {
                if let Some(position) = pending_comma.take() {
                    out.truncate(position);
                }
                out.push(char);
            }
            _ => {
                if !char.is_whitespace() {
                    pending_comma = None;
                }
                out.push(char);
            }
        }
        index += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strict_json_parses() {
        assert_eq!(
            parse_jsonc(r#"{"a": 1, "b": [true, null]}"#).unwrap(),
            json!({"a": 1, "b": [true, null]})
        );
    }

    #[test]
    fn comments_and_trailing_commas_allowed() {
        let text = r#"{
            // line comment
            /* block
               comment */
            "a": 1, // trailing comment
            "list": [1, 2, 3,],
            "obj": {"nested": "x",},
        }"#;
        assert_eq!(
            parse_jsonc(text).unwrap(),
            json!({"a": 1, "list": [1, 2, 3], "obj": {"nested": "x"}})
        );
    }

    #[test]
    fn comment_markers_inside_strings_are_literal() {
        // `//` inside the URL and commas inside the regex-ish value must
        // survive the comment/trailing-comma stripping untouched.
        assert_eq!(
            parse_jsonc(r#"{"url": "http://x//y", "re": "a,b,,", "q": "/* not a comment */"}"#)
                .unwrap(),
            json!({"url": "http://x//y", "re": "a,b,,", "q": "/* not a comment */"})
        );
    }

    #[test]
    fn json5_excesses_are_rejected_like_ref() {
        // ref jsonc-parser fills `errors` for each of these — the
        // document is rejected (G-281①: previously accepted by json5).
        assert!(parse_jsonc("{key: 1}").is_err(), "unquoted key");
        assert!(parse_jsonc("{'a': 1}").is_err(), "single-quoted string");
        assert!(parse_jsonc("{a: 0x10}").is_err(), "hex number");
        assert!(parse_jsonc("{a: +1}").is_err(), "leading plus");
        assert!(parse_jsonc("{a: Infinity}").is_err(), "Infinity");
        assert!(parse_jsonc("{a: NaN}").is_err(), "NaN");
        assert!(parse_jsonc("{a: .5}").is_err(), "leading-dot number");
        assert!(parse_jsonc("{a: 01}").is_err(), "leading zero");
        assert!(parse_jsonc(r#"{"a": "unterminated"#).is_err(), "unterminated string");
        assert!(parse_jsonc("{").is_err(), "unbalanced");
    }

    #[test]
    fn raw_control_chars_in_strings_are_tolerated() {
        // jsonc-parser's tolerant scan accepts raw tabs/newlines inside
        // strings; keep that acceptance (escaped for the strict pass).
        assert_eq!(parse_jsonc("{\"a\": \"x\ty\"}").unwrap(), json!({"a": "x\ty"}));
    }

    #[test]
    fn bom_is_stripped() {
        assert_eq!(parse_jsonc("\u{FEFF}{\"a\": 1}").unwrap(), json!({"a": 1}));
    }

    #[test]
    fn empty_document_rejects_like_ref() {
        // jsonc-parser errors on an empty document (value expected).
        assert!(parse_jsonc("").is_err());
        assert!(parse_jsonc("// only a comment").is_err());
    }
}
