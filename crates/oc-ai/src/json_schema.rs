//! A shared minimal JSON-Schema validator over the keyword subset the
//! built-in and MCP tool schemas use (originally the G-59 tool-input
//! validator in `oc-core/src/tool/registry.rs`, moved here so
//! `generateObject`'s output validation (G-100) reuses the exact same
//! engine — `oc-core` depends on `oc-ai`, never the reverse).
//!
//! Keywords: object `required`, per-property `type`, `enum`,
//! `minLength`/`maxLength`, `minimum`/`maximum`, nested object/array
//! `items`, plus the G-59 additions: `pattern`, `oneOf`/`anyOf`/`allOf`,
//! `$defs` + local `$ref` resolution, `minItems`/`maxItems`,
//! `additionalProperties` (bool + schema), and `uniqueItems`. `format`
//! stays lenient; unknown keywords are ignored (like effect's conversion
//! passthrough).

/// One validation issue (path + message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub path: String,
    pub message: String,
}

/// Validate `value` against `schema`, returning every issue found.
#[must_use]
pub fn validate_input(schema: &serde_json::Value, value: &serde_json::Value) -> Vec<Issue> {
    let mut issues = Vec::new();
    validate(schema, schema, value, "", &mut issues, 0);
    issues
}

/// Does a single JSON Schema `type` name match `value`?
fn matches_type(expected: &str, value: &serde_json::Value) -> bool {
    match (expected, value) {
        ("string", serde_json::Value::String(_)) => true,
        ("number" | "integer", serde_json::Value::Number(n)) => {
            expected == "number" || n.is_i64() || n.is_u64()
        }
        ("boolean", serde_json::Value::Bool(_)) => true,
        ("object", serde_json::Value::Object(_)) => true,
        ("array", serde_json::Value::Array(_)) => true,
        ("null", serde_json::Value::Null) => true,
        _ => false,
    }
}

fn type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn join_path(path: &str, key: &str) -> String {
    if path.is_empty() { key.to_string() } else { format!("{path}.{key}") }
}

fn custom_message(schema: &serde_json::Value) -> Option<&str> {
    schema.get("x-opencode-message").and_then(|v| v.as_str())
}

/// Local `#/...` JSON-pointer `$ref` resolution against the root schema
/// (covers `$defs` and any other in-document anchor).
fn resolve_ref<'a>(root: &'a serde_json::Value, reference: &str) -> Option<&'a serde_json::Value> {
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

/// Does `value` satisfy `schema`? (scratch validation, no side effects).
fn schema_matches(
    schema: &serde_json::Value,
    root: &serde_json::Value,
    value: &serde_json::Value,
    depth: usize,
) -> bool {
    let mut scratch = Vec::new();
    validate(schema, root, value, "", &mut scratch, depth + 1);
    scratch.is_empty()
}

#[allow(clippy::too_many_lines)] // one keyword arm each, mirroring ajv-style checks
fn validate(
    schema: &serde_json::Value,
    root: &serde_json::Value,
    value: &serde_json::Value,
    path: &str,
    issues: &mut Vec<Issue>,
    depth: usize,
) {
    // Cycle guard for recursive `$ref`s.
    if depth > 64 {
        return;
    }
    let Some(object) = schema.as_object() else { return };
    // Local `$ref`: validate against the referenced subschema; external
    // refs are ignored (lenient, like the MCP passthrough).
    if let Some(reference) = object.get("$ref").and_then(|v| v.as_str()) {
        if let Some(resolved) = resolve_ref(root, reference) {
            validate(resolved, root, value, path, issues, depth + 1);
        }
        return;
    }
    if let Some(list) = object.get("allOf").and_then(|v| v.as_array()) {
        for sub in list {
            validate(sub, root, value, path, issues, depth + 1);
        }
    }
    if let Some(list) = object.get("anyOf").and_then(|v| v.as_array()) {
        if !list.iter().any(|sub| schema_matches(sub, root, value, depth)) {
            issues.push(Issue {
                path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                message: "must match a schema in anyOf".to_string(),
            });
        }
    }
    if let Some(list) = object.get("oneOf").and_then(|v| v.as_array()) {
        let matches = list.iter().filter(|sub| schema_matches(sub, root, value, depth)).count();
        if matches != 1 {
            issues.push(Issue {
                path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                message: "must match exactly one schema in oneOf".to_string(),
            });
        }
    }
    // `type` accepts a single name or a union array (JSON Schema spec);
    // the union form is common in `generateObject` schemas.
    let expected_type = object
        .get("type")
        .map(|v| match v {
            serde_json::Value::String(name) => vec![name.clone()],
            serde_json::Value::Array(names) => {
                names.iter().filter_map(|name| name.as_str().map(str::to_string)).collect()
            }
            _ => Vec::new(),
        })
        .filter(|names| !names.is_empty());
    if let Some(expected) = expected_type {
        let matches = expected.iter().any(|expected| matches_type(expected.as_str(), value));
        if !matches {
            let expected = expected.join(" | ");
            let message = custom_message(schema)
                .map(str::to_string)
                .unwrap_or_else(|| format!("Expected {expected}, received {}", type_name(value)));
            issues.push(Issue {
                path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                message,
            });
            return;
        }
    }
    if let Some(min_length) = object.get("minLength").and_then(|v| v.as_u64()) {
        if let serde_json::Value::String(text) = value {
            if (text.chars().count() as u64) < min_length {
                let message = custom_message(schema)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("must NOT have fewer than {min_length} characters"));
                issues.push(Issue {
                    path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                    message,
                });
            }
        }
    }
    if let Some(max_length) = object.get("maxLength").and_then(|v| v.as_u64()) {
        if let serde_json::Value::String(text) = value {
            if (text.chars().count() as u64) > max_length {
                let message = custom_message(schema)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("must NOT have more than {max_length} characters"));
                issues.push(Issue {
                    path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                    message,
                });
            }
        }
    }
    if let Some(pattern) = object.get("pattern").and_then(|v| v.as_str()) {
        if let serde_json::Value::String(text) = value {
            // Invalid pattern → lenient skip (no schema error surface).
            if let Ok(regex) = regex::Regex::new(pattern) {
                if !regex.is_match(text) {
                    let message = custom_message(schema)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("must match pattern \"{pattern}\""));
                    issues.push(Issue {
                        path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                        message,
                    });
                }
            }
        }
    }
    if let Some(minimum) = object.get("minimum").and_then(|v| v.as_f64()) {
        if let serde_json::Value::Number(number) = value {
            if let Some(n) = number.as_f64() {
                if n < minimum {
                    issues.push(Issue {
                        path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                        message: format!("must be greater than or equal to {minimum}"),
                    });
                }
            }
        }
    }
    if let Some(maximum) = object.get("maximum").and_then(|v| v.as_f64()) {
        if let serde_json::Value::Number(number) = value {
            if let Some(n) = number.as_f64() {
                if n > maximum {
                    issues.push(Issue {
                        path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                        message: format!("must be less than or equal to {maximum}"),
                    });
                }
            }
        }
    }
    if let Some(enum_values) = object.get("enum").and_then(|v| v.as_array()) {
        if !enum_values.contains(value) {
            issues.push(Issue {
                path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                message: format!(
                    "must be equal to one of the allowed values: {}",
                    serde_json::to_string(enum_values).unwrap_or_default()
                ),
            });
        }
    }
    match value {
        serde_json::Value::Object(fields) => {
            // `required` applies regardless of `properties`.
            if let Some(required) = object.get("required").and_then(|v| v.as_array()) {
                for key in required {
                    if let Some(key) = key.as_str() {
                        if !fields.contains_key(key) {
                            issues.push(Issue {
                                path: if path.is_empty() {
                                    key.to_string()
                                } else {
                                    format!("{path}.{key}")
                                },
                                message: "is required".to_string(),
                            });
                        }
                    }
                }
            }
            // `additionalProperties` is enforced whether or not the
            // schema declares `properties`: without it the known-key set
            // is simply empty (a `const`/`enum` object form still fails
            // on extras through this same path).
            let properties = object.get("properties");
            let additional = object.get("additionalProperties");
            for (key, field) in fields {
                match properties.and_then(|props| props.get(key)) {
                    Some(field_schema) => {
                        validate(
                            field_schema,
                            root,
                            field,
                            &join_path(path, key),
                            issues,
                            depth + 1,
                        );
                    }
                    None => match additional {
                        // `false` → extra properties are rejected.
                        Some(serde_json::Value::Bool(false)) => issues.push(Issue {
                            path: join_path(path, key),
                            message: "must NOT have additional properties".to_string(),
                        }),
                        // Schema → validate the extra property against it.
                        Some(schema @ serde_json::Value::Object(_)) => {
                            validate(schema, root, field, &join_path(path, key), issues, depth + 1);
                        }
                        // Absent / `true` → open object.
                        _ => {}
                    },
                }
            }
        }
        serde_json::Value::Array(items) => {
            if let Some(min_items) = object.get("minItems").and_then(|v| v.as_u64()) {
                if (items.len() as u64) < min_items {
                    issues.push(Issue {
                        path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                        message: format!("must NOT have fewer than {min_items} items"),
                    });
                }
            }
            if let Some(max_items) = object.get("maxItems").and_then(|v| v.as_u64()) {
                if (items.len() as u64) > max_items {
                    issues.push(Issue {
                        path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                        message: format!("must NOT have more than {max_items} items"),
                    });
                }
            }
            if object.get("uniqueItems").and_then(|v| v.as_bool()) == Some(true) {
                let mut seen = Vec::with_capacity(items.len());
                if items.iter().any(|item| {
                    let dup = seen.contains(item);
                    seen.push(item.clone());
                    dup
                }) {
                    issues.push(Issue {
                        path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                        message: "must NOT have duplicate items".to_string(),
                    });
                }
            }
            if let Some(item_schema) = object.get("items") {
                for (index, item) in items.iter().enumerate() {
                    validate(
                        item_schema,
                        root,
                        item,
                        &format!("{path}[{index}]"),
                        issues,
                        depth + 1,
                    );
                }
            }
        }
        _ => {}
    }
}
