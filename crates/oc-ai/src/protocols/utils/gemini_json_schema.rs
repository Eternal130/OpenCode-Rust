//! Gemini tool-schema normalization (port of
//! `src/protocols/utils/gemini-json-schema.ts`).
//!
//! Gemini's `parametersJsonSchema` accepts standard JSON Schema, but rejects
//! a few shapes that published tool schemas commonly contain. Rewrite only
//! those and send everything else unchanged.

use serde_json::{Map, Value};

use super::super::sanitizer::VALUES;

/// Keys whose values are schema maps (rewrite each entry, recurse).
const SCHEMA_MAPS: [&str; 6] =
    ["properties", "patternProperties", "$defs", "definitions", "dependentSchemas", "dependencies"];

fn is_record(value: &Value) -> bool {
    value.is_object()
}

fn map_values(
    record: &Map<String, Value>,
    map: impl Fn(&Value, &str) -> Value,
) -> Map<String, Value> {
    record.iter().map(|(key, value)| (key.clone(), map(value, key))).collect()
}

/// `Number(x)` for the sibling checks below: numbers as-is, numeric strings
/// parsed, `true` → 1 — anything else (absent, null, objects) is `NaN`-like.
fn js_number(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(Value::Bool(true)) => Some(1.0),
        _ => None,
    }
}

/// Rewrite the shapes Gemini rejects: undeclared `required` names, Draft-04
/// boolean exclusive bounds, Draft-07 tuple arrays (`items` array +
/// `additionalItems` → `prefixItems` + `items`). Everything else recurses
/// unchanged.
fn normalize_node(schema: &Value) -> Value {
    match schema {
        Value::Array(items) => Value::Array(items.iter().map(normalize_node).collect()),
        Value::Object(record) => {
            let properties = record.get("properties").filter(|value| is_record(value));
            let mut out = Map::new();
            for (key, value) in record {
                if VALUES.contains(&key.as_str()) {
                    out.insert(key.clone(), value.clone());
                } else if SCHEMA_MAPS.contains(&key.as_str()) && is_record(value) {
                    out.insert(
                        key.clone(),
                        Value::Object(map_values(value.as_object().unwrap(), |child, _| {
                            normalize_node(child)
                        })),
                    );
                } else if key == "required" {
                    // `required` may only name declared properties.
                    match (properties, value.as_array()) {
                        (Some(properties), Some(names)) => {
                            let filtered: Vec<Value> = names
                                .iter()
                                .filter(|name| {
                                    name.as_str().is_some_and(|name| properties.get(name).is_some())
                                })
                                .cloned()
                                .collect();
                            out.insert(key.clone(), Value::Array(filtered));
                        }
                        _ => {
                            out.insert(key.clone(), normalize_node(value));
                        }
                    }
                } else if key == "exclusiveMinimum" {
                    match value {
                        Value::Bool(true) => {
                            // Draft-04 boolean exclusive bounds become the
                            // numeric form (the sibling bound's own value).
                            if let Some(minimum) = record.get("minimum").filter(|v| v.is_number()) {
                                out.insert(key.clone(), minimum.clone());
                            }
                        }
                        Value::Bool(false) => {}
                        _ => {
                            out.insert(key.clone(), normalize_node(value));
                        }
                    }
                } else if key == "exclusiveMaximum" {
                    match value {
                        Value::Bool(true) => {
                            if let Some(maximum) = record.get("maximum").filter(|v| v.is_number()) {
                                out.insert(key.clone(), maximum.clone());
                            }
                        }
                        Value::Bool(false) => {}
                        _ => {
                            out.insert(key.clone(), normalize_node(value));
                        }
                    }
                } else if key == "minimum"
                    && matches!(record.get("exclusiveMinimum"), Some(Value::Bool(true)))
                {
                    // Replaced by the numeric exclusiveMinimum above.
                } else if key == "maximum"
                    && matches!(record.get("exclusiveMaximum"), Some(Value::Bool(true)))
                {
                    // Replaced by the numeric exclusiveMaximum above.
                } else if key == "items" {
                    match value.as_array() {
                        // Draft-07 tuples (`items` array) are `prefixItems` in 2020-12.
                        Some(items) => {
                            out.insert(
                                "prefixItems".to_string(),
                                Value::Array(items.iter().map(normalize_node).collect()),
                            );
                        }
                        None => {
                            out.insert(key.clone(), normalize_node(value));
                        }
                    }
                } else if key == "additionalItems" {
                    if record.get("items").and_then(Value::as_array).is_some() {
                        out.insert("items".to_string(), normalize_node(value));
                    }
                } else {
                    out.insert(key.clone(), normalize_node(value));
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Gemini accepts a recursive `$ref` only when the loop passes through an
/// optional property or a potentially empty array `items`. Replace other
/// self-references with an unconstrained schema (`{}`).
fn cut_loops(schema: &Value, target: &str, safe: bool) -> Value {
    match schema {
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| cut_loops(item, target, safe)).collect())
        }
        Value::Object(record) => {
            if !safe && record.get("$ref").and_then(Value::as_str) == Some(target) {
                return Value::Object(Map::new());
            }
            let required: Vec<&str> = record
                .get("required")
                .and_then(Value::as_array)
                .map(|names| names.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            Value::Object(map_values(record, |value, key| {
                if VALUES.contains(&key) {
                    return value.clone();
                }
                if key == "items" {
                    // A potentially-empty array (no `minItems` > 0) is a safe
                    // recursion boundary for `$ref` loops.
                    let next_safe =
                        safe || !js_number(record.get("minItems")).is_some_and(|min| min > 0.0);
                    return cut_loops(value, target, next_safe);
                }
                if key == "properties" && is_record(value) {
                    return Value::Object(map_values(value.as_object().unwrap(), |child, name| {
                        cut_loops(child, target, safe || !required.contains(&name))
                    }));
                }
                if SCHEMA_MAPS.contains(&key) && is_record(value) {
                    return Value::Object(map_values(value.as_object().unwrap(), |child, _| {
                        cut_loops(child, target, safe)
                    }));
                }
                cut_loops(value, target, safe)
            }))
        }
        other => other.clone(),
    }
}

/// Normalize a tool input schema for Gemini's `parametersJsonSchema`
/// (port of `GeminiJsonSchema.normalize`): rewrite the rejected shapes, then
/// break `$ref` cycles that cannot pass through an optional boundary.
/// Non-object schemas collapse to `{}`.
#[must_use]
pub fn normalize(schema: &Value) -> Value {
    let normalized = normalize_node(schema);
    let Some(record) = normalized.as_object() else {
        return Value::Object(Map::new());
    };
    let seeded = map_values(record, |value, key| {
        if (key == "$defs" || key == "definitions") && is_record(value) {
            Value::Object(map_values(value.as_object().unwrap(), |def, name| {
                cut_loops(def, &format!("#/{key}/{name}"), false)
            }))
        } else {
            value.clone()
        }
    });
    let result = cut_loops(&Value::Object(seeded), "#", false);
    if is_record(&result) {
        result
    } else {
        Value::Object(Map::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The full "normalizes the JSON Schema shapes Gemini rejects" case from
    /// `test/provider/gemini.test.ts`, asserted at the projection level.
    #[test]
    fn normalizes_the_shapes_gemini_rejects() {
        let input = json!({
            "type": "object",
            "required": ["ttl", "items", "parent", "missing"],
            "properties": {
                "ttl": { "type": "number", "minimum": 0, "exclusiveMinimum": true, "maximum": 60, "exclusiveMaximum": false },
                "pair": { "type": "array", "items": [{ "type": "string" }, { "type": "integer" }], "additionalItems": false },
                "items": { "type": "array", "items": { "type": "string" } },
                "required": { "type": "boolean" },
                "params": { "$ref": "#/$defs/Json" },
                "tree": { "$ref": "#/$defs/Node" },
                "chain": { "$ref": "#/$defs/Link" },
                "batch": { "$ref": "#/$defs/Batch" },
                "self": { "$ref": "#" },
                "parent": { "$ref": "#" },
                "cart": { "type": "object", "default": { "items": ["a"] } }
            },
            "dependentRequired": { "items": ["ttl"] },
            "$defs": {
                "Json": {
                    "anyOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "$ref": "#/$defs/Json" } },
                        { "type": "object", "additionalProperties": { "$ref": "#/$defs/Json" } }
                    ]
                },
                "Node": { "type": "object", "properties": { "child": { "$ref": "#/$defs/Node" } } },
                "Link": { "type": "object", "required": ["next"], "properties": { "next": { "$ref": "#/$defs/Link" } } },
                "Batch": { "type": "array", "minItems": 1, "items": { "$ref": "#/$defs/Batch" } }
            }
        });
        let expected = json!({
            "type": "object",
            "required": ["ttl", "items", "parent"],
            "properties": {
                "ttl": { "type": "number", "exclusiveMinimum": 0, "maximum": 60 },
                "pair": { "type": "array", "prefixItems": [{ "type": "string" }, { "type": "integer" }], "items": false },
                "items": { "type": "array", "items": { "type": "string" } },
                "required": { "type": "boolean" },
                "params": { "$ref": "#/$defs/Json" },
                "tree": { "$ref": "#/$defs/Node" },
                "chain": { "$ref": "#/$defs/Link" },
                "batch": { "$ref": "#/$defs/Batch" },
                "self": { "$ref": "#" },
                "parent": {},
                "cart": { "type": "object", "default": { "items": ["a"] } }
            },
            "dependentRequired": { "items": ["ttl"] },
            "$defs": {
                "Json": {
                    "anyOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "$ref": "#/$defs/Json" } },
                        { "type": "object", "additionalProperties": {} }
                    ]
                },
                "Node": { "type": "object", "properties": { "child": { "$ref": "#/$defs/Node" } } },
                "Link": { "type": "object", "required": ["next"], "properties": { "next": {} } },
                "Batch": { "type": "array", "minItems": 1, "items": {} }
            }
        });
        assert_eq!(normalize(&input), expected);
    }

    #[test]
    fn passes_standard_json_schema_through_unchanged() {
        let input = json!({
            "type": "object",
            "required": ["level"],
            "properties": {
                "level": { "type": "integer", "enum": [1, 2, 3] },
                "status": { "type": ["number", "string"], "description": "Status filter" },
                "maybe": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
                "kind": { "const": "fixed" },
                "mode": { "enum": ["fast", "safe"] },
                "tags": { "type": "array" },
                "env": { "type": "object", "additionalProperties": { "type": "string" } },
                "action": {
                    "oneOf": [
                        { "type": "object", "properties": { "type": { "const": "move" }, "to": { "type": "string" } } },
                        { "type": "object", "properties": { "type": { "const": "delete" } } }
                    ]
                },
                "item": { "$ref": "#/$defs/Item" }
            },
            "$defs": { "Item": { "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] } }
        });
        assert_eq!(normalize(&input), input);
    }

    #[test]
    fn keeps_nested_empty_objects() {
        let input = json!({"type": "object", "required": ["options"], "properties": {"options": {"type": "object", "description": "Optional provider settings", "properties": {}}}});
        assert_eq!(normalize(&input), input);
    }

    #[test]
    fn required_without_properties_is_kept_verbatim() {
        let input = json!({"type": "object", "required": ["a"]});
        assert_eq!(normalize(&input), input);
    }

    #[test]
    fn non_object_roots_collapse_to_empty_objects() {
        assert_eq!(normalize(&json!("string")), json!({}));
        assert_eq!(normalize(&json!([1, 2])), json!({}));
    }

    #[test]
    fn exclusive_boolean_false_drops_the_bound_entirely() {
        let input = json!({"type": "number", "maximum": 10, "exclusiveMaximum": false});
        assert_eq!(normalize(&input), json!({"type": "number", "maximum": 10}));
    }

    #[test]
    fn a_direct_root_self_reference_is_cut() {
        assert_eq!(normalize(&json!({"$ref": "#"})), json!({}));
    }
}
