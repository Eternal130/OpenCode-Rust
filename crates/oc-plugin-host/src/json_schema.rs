//! G-296 — a compact JSON-Schema subset validator for the plugin-RPC
//! surface (`rpc.invalid_input` / `rpc.invalid_output` /
//! declared-error data validation), mirroring the repo's two existing
//! precedents (oc-ai's `json_schema.rs`, originally oc-core's G-59
//! tool-input validator). `oc-plugin-host` cannot depend on `oc-core`
//! or `oc-ai` (they depend on this crate's embedder graph), so the
//! engine is duplicated here over the same keyword subset:
//!
//! `type` (single + union), `enum`, `const`, `required`,
//! `properties`, `additionalProperties`, `items`, `minItems`/
//! `maxItems`, `uniqueItems`, `minLength`/`maxLength`, `minimum`/
//! `maximum`, `pattern`, `oneOf`/`anyOf`/`allOf`, `$defs` + local
//! `$ref`. `format` stays lenient; unknown keywords are ignored.
//!
//! A schema that is not a JSON object, or an object with none of the
//! recognized keywords (e.g. the method-name skeleton an Effect
//! Schema instance degrades to over JSON), validates everything —
//! the lax fallback that keeps skeleton definitions dispatching.

use serde_json::Value;

/// One validation issue (path + message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub path: String,
    pub message: String,
}

/// Does a single JSON-Schema `type` name match `value`?
fn matches_type(expected: &str, value: &Value) -> bool {
    match (expected, value) {
        ("string", Value::String(_)) => true,
        ("number" | "integer", Value::Number(n)) => {
            expected == "number" || n.is_i64() || n.is_u64()
        }
        ("boolean", Value::Bool(_)) => true,
        ("object", Value::Object(_)) => true,
        ("array", Value::Array(_)) => true,
        ("null", Value::Null) => true,
        _ => false,
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn at_root(path: &str) -> String {
    if path.is_empty() { "root".to_string() } else { path.to_string() }
}

fn join_path(path: &str, key: &str) -> String {
    if path.is_empty() { key.to_string() } else { format!("{path}.{key}") }
}

/// The recognized keyword set — a schema object with NONE of these is
/// not a JSON schema (an Effect-Schema skeleton) and validates
/// everything (the lax fallback).
const KEYWORDS: &[&str] = &[
    "type", "enum", "const", "required", "properties", "additionalProperties", "items",
    "minItems", "maxItems", "uniqueItems", "minLength", "maxLength", "minimum", "maximum",
    "pattern", "oneOf", "anyOf", "allOf", "$ref", "$defs",
];

/// Is `schema` a JSON-Schema-looking object the validator should
/// enforce? (Skeletons and non-objects validate everything.)
#[must_use]
pub fn is_enforced_schema(schema: &Value) -> bool {
    let Some(object) = schema.as_object() else { return false };
    object.keys().any(|key| KEYWORDS.contains(&key.as_str()))
}

/// Local `#/...` JSON-pointer `$ref` resolution against the root
/// schema (covers `$defs` and any other in-document anchor).
fn resolve_ref<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    if pointer.is_empty() {
        return Some(root);
    }
    let pointer = pointer.strip_prefix('/')?;
    let mut current = root;
    for raw in pointer.split('/') {
        let token = raw.replace("~1", "/").replace("~0", "~");
        let next = current.as_object().and_then(|map| map.get(&token)).or_else(|| {
            let index = token.parse::<usize>().ok()?;
            current.as_array().and_then(|items| items.get(index))
        })?;
        current = next;
    }
    Some(current)
}

/// Does `value` satisfy `schema`? (scratch validation, no effects)
fn schema_matches(schema: &Value, root: &Value, value: &Value, depth: usize) -> bool {
    let mut scratch = Vec::new();
    validate(schema, root, value, "", &mut scratch, depth + 1);
    scratch.is_empty()
}

/// Validate `value` against `schema`, returning every issue found.
/// Non-schema values (see [`is_enforced_schema`]) return no issues.
#[must_use]
pub fn validate_input(schema: &Value, value: &Value) -> Vec<Issue> {
    if !is_enforced_schema(schema) {
        return Vec::new();
    }
    let mut issues = Vec::new();
    validate(schema, schema, value, "", &mut issues, 0);
    issues
}

/// The first issue's message (the failure surfaces one line, like the
/// ref's `errorMessage` join); `None` when valid.
#[must_use]
pub fn first_issue(schema: &Value, value: &Value) -> Option<String> {
    let issues = validate_input(schema, value);
    issues.first().map(|issue| {
        if issue.path.is_empty() || issue.path == "root" {
            issue.message.clone()
        } else {
            format!("{} {}", issue.path, issue.message)
        }
    })
}

#[allow(clippy::too_many_lines)] // one keyword arm each, mirroring the oc-ai engine
fn validate(
    schema: &Value,
    root: &Value,
    value: &Value,
    path: &str,
    issues: &mut Vec<Issue>,
    depth: usize,
) {
    // Cycle guard for recursive `$ref`s.
    if depth > 64 {
        return;
    }
    let Some(object) = schema.as_object() else { return };
    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        if let Some(resolved) = resolve_ref(root, reference) {
            validate(resolved, root, value, path, issues, depth + 1);
        }
        return;
    }
    if let Some(list) = object.get("allOf").and_then(Value::as_array) {
        for sub in list {
            validate(sub, root, value, path, issues, depth + 1);
        }
    }
    if let Some(list) = object.get("anyOf").and_then(Value::as_array) {
        if !list.iter().any(|sub| schema_matches(sub, root, value, depth)) {
            issues.push(Issue { path: at_root(path), message: "must match a schema in anyOf".into() });
        }
    }
    if let Some(list) = object.get("oneOf").and_then(Value::as_array) {
        let matches = list.iter().filter(|sub| schema_matches(sub, root, value, depth)).count();
        if matches != 1 {
            issues.push(Issue {
                path: at_root(path),
                message: "must match exactly one schema in oneOf".into(),
            });
        }
    }
    // `type` accepts a single name or a union array.
    let expected_type = object
        .get("type")
        .map(|v| match v {
            Value::String(name) => vec![name.clone()],
            Value::Array(names) => {
                names.iter().filter_map(|name| name.as_str().map(str::to_string)).collect()
            }
            _ => Vec::new(),
        })
        .filter(|names| !names.is_empty());
    if let Some(expected) = expected_type {
        let matches = expected.iter().any(|expected| matches_type(expected, value));
        if !matches {
            issues.push(Issue {
                path: at_root(path),
                message: format!("Expected {}, received {}", expected.join(" | "), type_name(value)),
            });
            return;
        }
    }
    if let Some(minimum) = object.get("minimum").and_then(Value::as_f64) {
        if let Value::Number(number) = value {
            if number.as_f64().is_some_and(|n| n < minimum) {
                issues.push(Issue {
                    path: at_root(path),
                    message: format!("must be greater than or equal to {minimum}"),
                });
            }
        }
    }
    if let Some(maximum) = object.get("maximum").and_then(Value::as_f64) {
        if let Value::Number(number) = value {
            if number.as_f64().is_some_and(|n| n > maximum) {
                issues.push(Issue {
                    path: at_root(path),
                    message: format!("must be less than or equal to {maximum}"),
                });
            }
        }
    }
    if let Some(min_length) = object.get("minLength").and_then(Value::as_u64) {
        if let Value::String(text) = value {
            if (text.chars().count() as u64) < min_length {
                issues.push(Issue {
                    path: at_root(path),
                    message: format!("must NOT have fewer than {min_length} characters"),
                });
            }
        }
    }
    if let Some(max_length) = object.get("maxLength").and_then(Value::as_u64) {
        if let Value::String(text) = value {
            if (text.chars().count() as u64) > max_length {
                issues.push(Issue {
                    path: at_root(path),
                    message: format!("must NOT have more than {max_length} characters"),
                });
            }
        }
    }
    if let Some(pattern) = object.get("pattern").and_then(Value::as_str) {
        if let Value::String(text) = value {
            // Invalid pattern → lenient skip (no schema error surface).
            if let Ok(regex) = regex::Regex::new(pattern) {
                if !regex.is_match(text) {
                    issues.push(Issue {
                        path: at_root(path),
                        message: format!("must match pattern \"{pattern}\""),
                    });
                }
            }
        }
    }
    if let Some(expected) = object.get("const") {
        if value != expected {
            issues.push(Issue {
                path: at_root(path),
                message: format!("must be equal to constant {}", expected),
            });
        }
    }
    if let Some(enum_values) = object.get("enum").and_then(Value::as_array) {
        if !enum_values.contains(value) {
            issues.push(Issue {
                path: at_root(path),
                message: format!(
                    "must be equal to one of the allowed values: {}",
                    serde_json::to_string(enum_values).unwrap_or_default()
                ),
            });
        }
    }
    match value {
        Value::Object(fields) => {
            if let Some(required) = object.get("required").and_then(Value::as_array) {
                for key in required {
                    if let Some(key) = key.as_str() {
                        if !fields.contains_key(key) {
                            issues.push(Issue {
                                path: join_path(path, key),
                                message: "is required".into(),
                            });
                        }
                    }
                }
            }
            let properties = object.get("properties");
            let additional = object.get("additionalProperties");
            for (key, field) in fields {
                match properties.and_then(|props| props.get(key)) {
                    Some(field_schema) => {
                        validate(field_schema, root, field, &join_path(path, key), issues, depth + 1);
                    }
                    None => match additional {
                        Some(Value::Bool(false)) => issues.push(Issue {
                            path: join_path(path, key),
                            message: "must NOT have additional properties".into(),
                        }),
                        Some(schema @ Value::Object(_)) => {
                            validate(schema, root, field, &join_path(path, key), issues, depth + 1);
                        }
                        _ => {}
                    },
                }
            }
        }
        Value::Array(items) => {
            if let Some(min_items) = object.get("minItems").and_then(Value::as_u64) {
                if (items.len() as u64) < min_items {
                    issues.push(Issue {
                        path: at_root(path),
                        message: format!("must NOT have fewer than {min_items} items"),
                    });
                }
            }
            if let Some(max_items) = object.get("maxItems").and_then(Value::as_u64) {
                if (items.len() as u64) > max_items {
                    issues.push(Issue {
                        path: at_root(path),
                        message: format!("must NOT have more than {max_items} items"),
                    });
                }
            }
            if object.get("uniqueItems").and_then(Value::as_bool) == Some(true) {
                let mut seen = Vec::with_capacity(items.len());
                if items.iter().any(|item| {
                    let dup = seen.contains(item);
                    seen.push(item.clone());
                    dup
                }) {
                    issues.push(Issue {
                        path: at_root(path),
                        message: "must NOT have duplicate items".into(),
                    });
                }
            }
            if let Some(item_schema) = object.get("items") {
                for (index, item) in items.iter().enumerate() {
                    validate(item_schema, root, item, &format!("{path}[{index}]"), issues, depth + 1);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn skeletons_and_non_objects_validate_everything() {
        assert!(validate_input(&json!({}), &json!("anything")).is_empty());
        assert!(validate_input(&json!("not an object"), &json!(null)).is_empty());
        assert!(validate_input(&json!(null), &json!({ "x": 1 })).is_empty());
    }

    #[test]
    fn required_and_types() {
        let schema = json!({
            "type": "object",
            "properties": { "n": { "type": "number" }, "s": { "type": "string" } },
            "required": ["n"],
            "additionalProperties": false,
        });
        assert!(validate_input(&schema, &json!({ "n": 1, "s": "x" })).is_empty());
        assert!(validate_input(&schema, &json!({ "s": "x" })).iter().any(|i| i.path == "n"));
        assert!(validate_input(&schema, &json!({ "n": 1, "extra": true }))
            .iter()
            .any(|i| i.message.contains("additional")));
        assert!(validate_input(&schema, &json!({ "n": "no" })).iter().any(|i| i.path == "n"));
    }

    #[test]
    fn enum_const_bounds_pattern() {
        assert!(!validate_input(&json!({ "enum": ["a", "b"] }), &json!("c")).is_empty());
        assert!(!validate_input(&json!({ "const": 7 }), &json!(8)).is_empty());
        assert!(!validate_input(&json!({ "minimum": 3 }), &json!(2)).is_empty());
        assert!(!validate_input(&json!({ "maximum": 3 }), &json!(4)).is_empty());
        assert!(!validate_input(&json!({ "pattern": "^x+$" }), &json!("xy")).is_empty());
        assert!(!validate_input(&json!({ "minLength": 2 }), &json!("x")).is_empty());
        assert!(!validate_input(&json!({ "maxLength": 2 }), &json!("xyz")).is_empty());
    }

    #[test]
    fn arrays_items_and_uniqueness() {
        assert!(!validate_input(&json!({ "minItems": 2 }), &json!([1])).is_empty());
        assert!(
            !validate_input(&json!({ "items": { "type": "integer" }, "maxItems": 2 }), &json!([1, "x", 3]))
                .is_empty()
        );
        assert!(!validate_input(&json!({ "uniqueItems": true }), &json!([1, 1])).is_empty());
    }

    #[test]
    fn combinators_and_refs() {
        assert!(validate_input(
            &json!({ "anyOf": [{ "type": "string" }, { "type": "number" }] }),
            &json!(5)
        )
        .is_empty());
        assert!(!validate_input(
            &json!({ "oneOf": [{ "type": "string" }, { "minLength": 1 }] }),
            &json!("both match")
        )
        .is_empty());
        let schema = json!({
            "$defs": { "positive": { "type": "number", "minimum": 0 } },
            "properties": { "n": { "$ref": "#/$defs/positive" } },
        });
        assert!(validate_input(&schema, &json!({ "n": 1 })).is_empty());
        assert!(!validate_input(&schema, &json!({ "n": -1 })).is_empty());
    }

    #[test]
    fn union_type_arrays() {
        let schema = json!({ "type": ["string", "null"] });
        assert!(validate_input(&schema, &json!(null)).is_empty());
        assert!(validate_input(&schema, &json!("x")).is_empty());
        assert!(!validate_input(&schema, &json!(3)).is_empty());
    }
}
