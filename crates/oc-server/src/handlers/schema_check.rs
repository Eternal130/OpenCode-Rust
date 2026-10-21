//! Effect-schema validation for the routes the Phase-10 error-path
//! battery pinned (P10-E1 payload / P10-E2 query): the port's error
//! bodies must be byte-equal to v2's `SchemaErrorMiddleware` output
//! (`middleware/schema-error.ts` → `InvalidRequestError{message, kind}`).
//!
//! v2 validates EVERY route's payload/query against the Effect schema
//! before the handler runs; the port validates where the battery (and
//! real clients) actually trip it — `session.create`'s payload and
//! `session.list`'s query — reproducing Effect's `TreeFormatter`
//! message shape exactly:
//!
//! ```text
//! Expected string | null\n  at ["title"]            (payload field type)
//! Expected Model.Ref | null\n  at ["model"]          (annotated schema id)
//! Expected string\n  at ["model"]["providerID"]     (nested path)
//! Expected object                                (non-object payload root)
//! Expected an integer\n  at ["limit"]               (NumberFromString → Int)
//! Expected a value greater than 0\n  at ["limit"]    (PositiveInt filter)
//! Expected "asc" | "desc"\n  at ["order"]            (literal union)
//! ```
//!
//! Verified field-by-field against a live v2.0.24 reference boot (see
//! `tools/parity/goldens/p10/VERIFICATION.md`). Multi-field failures
//! report the FIRST offending field in schema-declaration order, which
//! matches the reference for probed inputs. Unknown fields are ignored
//! (Effect strips excess properties).

use serde_json::Value;

/// One payload field: name, the `TreeFormatter` type label Effect prints
/// when the whole field is the wrong shape, and its validator.
struct FieldSpec {
    name: &'static str,
    /// Label for a whole-field mismatch (annotated schemas print their
    /// identifier, plain ones print the primitive).
    label: &'static str,
    kind: FieldKind,
}

enum FieldKind {
    /// `Schema.String` — nullable strings.
    String,
    /// A struct with an identifier annotation — must be an object, then
    /// each named member must be a string.
    Struct(&'static [&'static str]),
    /// A record — must be an object.
    Record,
    /// An array schema with an identifier annotation.
    Array,
}

/// The `session.create` payload struct (`groups/session.ts`
/// `HttpApiEndpoint.post("session.create")`), in declaration order.
const SESSION_CREATE_FIELDS: &[FieldSpec] = &[
    FieldSpec { name: "id", label: "string", kind: FieldKind::String },
    FieldSpec { name: "parentID", label: "string", kind: FieldKind::String },
    FieldSpec { name: "title", label: "string", kind: FieldKind::String },
    FieldSpec { name: "agent", label: "string", kind: FieldKind::String },
    FieldSpec {
        name: "model",
        label: "Model.Ref",
        kind: FieldKind::Struct(&["id", "providerID", "variant"]),
    },
    FieldSpec {
        name: "location",
        label: "Location.PublicRef",
        kind: FieldKind::Struct(&["directory"]),
    },
    FieldSpec { name: "metadata", label: "Session.Metadata", kind: FieldKind::Record },
    FieldSpec { name: "permissions", label: "Permission.Ruleset", kind: FieldKind::Array },
];

/// Effect `TreeFormatter` message for a failed field: expectation line +
/// two-space-indented path line.
fn field_error(label: &str, name: &str) -> String {
    format!("Expected {label} | null\n  at [\"{name}\"]")
}

fn member_error(name: &str, member: &str) -> String {
    format!("Expected string\n  at [\"{name}\"][\"{member}\"]")
}

/// Validates a `session.create` payload; `Err(message)` carries the
/// Effect message for the first offending field.
pub fn session_create_payload(body: &Value) -> Result<(), String> {
    let Some(object) = body.as_object() else {
        return Err("Expected object".to_owned());
    };
    for field in SESSION_CREATE_FIELDS {
        let Some(value) = object.get(field.name) else { continue };
        if value.is_null() {
            continue;
        }
        match field.kind {
            FieldKind::String => {
                if !value.is_string() {
                    return Err(field_error(field.label, field.name));
                }
            }
            FieldKind::Struct(members) => {
                let Some(member_object) = value.as_object() else {
                    return Err(field_error(field.label, field.name));
                };
                for member in members {
                    match member_object.get(*member) {
                        None | Some(Value::Null) => {}
                        Some(member_value) if member_value.is_string() => {}
                        Some(_) => return Err(member_error(field.name, member)),
                    }
                }
            }
            FieldKind::Record => {
                if !value.is_object() {
                    return Err(field_error(field.label, field.name));
                }
            }
            FieldKind::Array => {
                if !value.is_array() {
                    return Err(field_error(field.label, field.name));
                }
            }
        }
    }
    Ok(())
}

/// Validates the `session.list` query (`SessionsQueryFields`:
/// `limit: NumberFromString → PositiveInt`, `order: "asc" | "desc"`).
/// The raw query string (no leading `?`); values percent-decoded.
pub fn session_list_query(query: &str) -> Result<(), String> {
    for (key, value) in parse_query_pairs(query) {
        match key.as_str() {
            "limit" => check_limit(&value)?,
            "order" if value != "asc" && value != "desc" => {
                return Err("Expected \"asc\" | \"desc\"\n  at [\"order\"]".to_owned());
            }
            _ => {}
        }
    }
    Ok(())
}

/// `Schema.NumberFromString.pipe(decodeTo(PositiveInt))`: a non-numeric
/// or fractional string fails the Int check (`Expected an integer`);
/// an integer ≤ 0 fails the positivity filter.
fn check_limit(value: &str) -> Result<(), String> {
    let number = js_number(value);
    if !number.is_finite() || number.fract() != 0.0 {
        return Err("Expected an integer\n  at [\"limit\"]".to_owned());
    }
    if number <= 0.0 {
        return Err("Expected a value greater than 0\n  at [\"limit\"]".to_owned());
    }
    Ok(())
}

/// JS `Number(string)` for the inputs that matter: trims whitespace,
/// `""` → 0, decimal/exponent literals, `0x` hex, `Infinity`; anything
/// else is `NaN`. `pub(crate)`: the HTTP-H0 middleware seam reuses this
/// for its NumberFromString query params (single implementation).
pub(crate) fn js_number(raw: &str) -> f64 {
    let text = raw.trim_matches(|c: char| c.is_ascii_whitespace());
    if text.is_empty() {
        return 0.0;
    }
    let (sign, text) = match text.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, text.strip_prefix('+').unwrap_or(text)),
    };
    let text = text.strip_prefix('+').unwrap_or(text);
    if text == "Infinity" {
        return sign * f64::INFINITY;
    }
    let lower = text.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix("0x") {
        return sign
            * u64::from_str_radix(
                &hex.chars().filter(char::is_ascii_hexdigit).collect::<String>(),
                16,
            )
            .map_or(f64::NAN, |value| {
                #[allow(clippy::cast_precision_loss)] // JS numbers ARE f64
                let value = value as f64;
                value
            });
    }
    if lower.ends_with("infinity") || lower.starts_with("0b") || lower.starts_with("0o") {
        return f64::NAN;
    }
    let cleaned: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        .collect();
    if cleaned.is_empty() || cleaned != text {
        // The filter only keeps shape-valid characters; a change means
        // stray characters → NaN (mirrors Number()'s rejection).
        return f64::NAN;
    }
    text.parse::<f64>().unwrap_or(f64::NAN) * sign
}

/// Minimal query-string split: `&`-separated `k=v` pairs with
/// percent-decoding (query keys/values in play are plain ASCII).
/// `pub(crate)`: the HTTP-H0 middleware seam reuses this for its query
/// interpreter (same percent-decoding, single implementation).
pub(crate) fn parse_query_pairs(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => (decode_component(key), decode_component(value)),
            None => (decode_component(pair), String::new()),
        })
        .collect()
}

fn decode_component(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload_err(body: &serde_json::Value) -> String {
        session_create_payload(body).unwrap_err()
    }

    fn query_err(query: &str) -> String {
        session_list_query(query).unwrap_err()
    }

    // -- payload: the exact reference shapes (live-boot verified) ---------

    #[test]
    fn payload_wrongly_typed_string_field() {
        assert_eq!(
            payload_err(&json!({"title": 12345})),
            "Expected string | null\n  at [\"title\"]"
        );
    }

    #[test]
    fn payload_first_field_in_declaration_order_wins() {
        assert_eq!(
            payload_err(&json!({"title": 1, "parentID": 2})),
            "Expected string | null\n  at [\"parentID\"]"
        );
    }

    #[test]
    fn payload_annotated_schema_labels() {
        assert_eq!(
            payload_err(&json!({"model": "notanobject"})),
            "Expected Model.Ref | null\n  at [\"model\"]"
        );
        assert_eq!(
            payload_err(&json!({"metadata": 5})),
            "Expected Session.Metadata | null\n  at [\"metadata\"]"
        );
        assert_eq!(
            payload_err(&json!({"location": 5})),
            "Expected Location.PublicRef | null\n  at [\"location\"]"
        );
        assert_eq!(
            payload_err(&json!({"permissions": 5})),
            "Expected Permission.Ruleset | null\n  at [\"permissions\"]"
        );
        assert_eq!(
            payload_err(&json!({"permissions": {}})),
            "Expected Permission.Ruleset | null\n  at [\"permissions\"]"
        );
    }

    #[test]
    fn payload_nested_struct_members() {
        assert_eq!(
            payload_err(&json!({"model": {"id": "x", "providerID": 5}})),
            "Expected string\n  at [\"model\"][\"providerID\"]"
        );
        assert_eq!(
            payload_err(&json!({"location": {"directory": 5}})),
            "Expected string\n  at [\"location\"][\"directory\"]"
        );
    }

    #[test]
    fn payload_non_object_root() {
        assert_eq!(payload_err(&json!([1, 2])), "Expected object");
        assert_eq!(payload_err(&json!("text")), "Expected object");
    }

    #[test]
    fn payload_accepts_nulls_and_unknowns() {
        assert!(session_create_payload(&json!({"title": null, "parentID": null})).is_ok());
        assert!(session_create_payload(&json!({"bogus": "x"})).is_ok());
        assert!(session_create_payload(&json!({})).is_ok());
        assert!(session_create_payload(&json!({
            "id": "ses_1", "parentID": "ses_0", "title": "t", "agent": "build",
            "model": {"providerID": "p", "id": "m", "variant": "default"},
            "location": {"directory": "/tmp"},
            "metadata": {"k": [1, {"nested": true}]},
            "permissions": [{"action": "*", "resource": "*", "effect": "allow"}],
        }))
        .is_ok());
    }

    // -- query: the exact reference shapes --------------------------------

    #[test]
    fn query_limit_shapes() {
        assert_eq!(query_err("limit=notanumber"), "Expected an integer\n  at [\"limit\"]");
        assert_eq!(query_err("limit=5.5"), "Expected an integer\n  at [\"limit\"]");
        assert_eq!(query_err("limit=0"), "Expected a value greater than 0\n  at [\"limit\"]");
        assert_eq!(query_err("limit="), "Expected a value greater than 0\n  at [\"limit\"]");
        assert_eq!(query_err("limit=-3"), "Expected a value greater than 0\n  at [\"limit\"]");
    }

    #[test]
    fn query_order_literal_union() {
        assert_eq!(query_err("order=bogus"), "Expected \"asc\" | \"desc\"\n  at [\"order\"]");
        assert_eq!(
            query_err("limit=3&order=bogus"),
            "Expected \"asc\" | \"desc\"\n  at [\"order\"]"
        );
    }

    #[test]
    fn query_accepts_valid_and_ignores_unknown() {
        assert!(session_list_query("").is_ok());
        assert!(session_list_query("limit=50&order=asc").is_ok());
        assert!(session_list_query("limit=%31&order=desc").is_ok());
        assert!(session_list_query("bogus=1&directory=/x&search=y").is_ok());
    }

    #[test]
    fn js_number_semantics() {
        assert_eq!(js_number(""), 0.0);
        assert_eq!(js_number(" 5 "), 5.0);
        assert!(js_number("notanumber").is_nan());
        assert_eq!(js_number("0x10"), 16.0);
        assert_eq!(js_number("1e2"), 100.0);
        assert_eq!(js_number("-2"), -2.0);
        assert_eq!(js_number("Infinity"), f64::INFINITY);
    }
}
