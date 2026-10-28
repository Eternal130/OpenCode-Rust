//! Port of `plugin/tool-input-repair.ts` (v2.0.24) — the builtin
//! `opencode.tool.input.repair` plugin.
//!
//! Repairs apply only when the input schema unambiguously supports them
//! (ref `plugin/tool-input-repair.ts:3-13`):
//! - Stringified root or nested object: `{"limit":"20"}` -> `{ limit: 20 }`
//! - Closed object: `{ limit: "20", extra: true }` -> `{ limit: 20 }`
//! - Optional null or empty-object placeholder: `{ limit: null }` -> `{}`
//! - Numeric or boolean string: `{ limit: "20", enabled: "false" }`
//!   -> `{ limit: 20, enabled: false }`
//! - Nullable field: `{ count: "2" }` -> `{ count: 2 }`
//! - Stringified array or compatible item: `{ tags: '["a"]', count: "2" }`
//!   -> `{ tags: ["a"], count: [2] }`
//! - Positional tuple: `{ pair: ["2", "false"] }` -> `{ pair: [2, false] }`
//! - Typed dictionary: `{ counts: { first: "2" } }` -> `{ counts: { first: 2 } }`
//! - Nested fields and local references: `{ items: [{ count: "2" }] }`
//!   -> `{ items: [{ count: 2 }] }`
//!
//! NOTE: `oc-ai/partial_json.rs` and `oc-ai/sanitize.rs` are NOT
//! equivalents — this module owns tool-input coercion at the
//! `tool.execute.before` seam.

use serde_json::{Map, Value};

use crate::hooks::{HookOptions, InternalPlugin, ToolExecuteBeforeEvent};
use std::sync::Arc;

/// `maxDepth` (ref tool-input-repair.ts:17).
const MAX_DEPTH: usize = 6;

/// The registration step shared by the plugin closure and focused tests.
pub(crate) fn install_hook(
    hooks: &Arc<crate::hooks::HookRegistry>,
    tools: &Arc<crate::location::ToolRegistryState>,
) -> crate::hooks::Registration {
    let tools = Arc::clone(tools);
    let handler = crate::hooks::fn_hook(move |event: &mut ToolExecuteBeforeEvent| {
        if event.tool == "execute" {
            return Ok(());
        }
        let Some(tool) = tools.get(&event.tool) else {
            return Ok(());
        };
        let schema = tool.input;
        if schema.get("type").and_then(Value::as_str) != Some("object") {
            return Ok(());
        }
        event.input = repair(&event.input, &schema, &schema, 0);
        Ok(())
    });
    // Registrations live for the location lifetime (dropped with the
    // graph), like the patch plugin's codemod hooks.
    futures::executor::block_on(
        hooks.register_tool_execute_before(handler, HookOptions::default()),
    )
}

/// The builtin plugin: hooks `execute.before` and repairs the input against
/// the tool's object input schema (ref tool-input-repair.ts:19-32).
///
/// The outer Code Mode tool is built per snapshot rather than registered,
/// so its trivial `{ code }` input is skipped by name; the tools it calls
/// are repaired normally.
#[must_use]
pub fn plugin() -> InternalPlugin {
    InternalPlugin::new("opencode.tool.input.repair", |hooks, ports| {
        std::mem::forget(install_hook(hooks, ports.tools));
        Ok(())
    })
}

/// `repair` (ref tool-input-repair.ts:34-66).
#[must_use]
pub fn repair(value: &Value, schema: &Value, root: &Value, depth: usize) -> Value {
    if depth > MAX_DEPTH {
        return value.clone();
    }

    // Local `$ref` into `$defs` / `definitions`.
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let definitions = if is_defs_ref(reference, "#/$defs/") {
            root.get("$defs")
        } else if is_defs_ref(reference, "#/definitions/") {
            root.get("definitions")
        } else {
            None
        };
        let Some(definitions) = definitions.and_then(Value::as_object) else {
            return value.clone();
        };
        let name = unescape_pointer(reference.rsplit('/').next().unwrap_or(""));
        let Some(target) = definitions.get(&name).filter(|target| target.is_object()) else {
            return value.clone();
        };
        return repair(value, target, root, depth + 1);
    }

    match schema.get("type") {
        // Union type: keep null when accepted, keep matching members, and
        // recurse only when a single non-null candidate remains.
        Some(Value::Array(types)) => {
            if value.is_null() && types.iter().any(|ty| ty == &Value::String("null".into())) {
                return value.clone();
            }
            if types.iter().any(|ty| ty.as_str() == Some(js_type(value))) {
                return value.clone();
            }
            let candidates: Vec<&Value> =
                types.iter().filter(|ty| ty.as_str() != Some("null")).collect();
            if candidates.len() == 1 {
                let mut narrowed = schema.clone();
                narrowed.insert("type".into(), candidates[0].clone());
                return repair(value, &narrowed, root, depth + 1);
            }
            value.clone()
        }
        None => {
            if schema.get("anyOf").is_some_and(Value::is_array)
                && schema.get("oneOf").is_some_and(Value::is_array)
            {
                return value.clone();
            }
            let branches = match (schema.get("anyOf"), schema.get("oneOf")) {
                (Some(Value::Array(items)), _) => Some(items),
                (None, Some(Value::Array(items))) => Some(items),
                _ => None,
            };
            let Some(branches) = branches else {
                return value.clone();
            };
            if value.is_null() {
                return value.clone();
            }
            if branches.iter().any(|branch| {
                !branch.is_object() || branch.get("type").and_then(Value::as_str) == Some(js_type(value))
            }) {
                return value.clone();
            }
            let candidates: Vec<&Value> = branches
                .iter()
                .filter(|branch| branch.is_object() && branch.get("type").and_then(Value::as_str) != Some("null"))
                .collect();
            if candidates.len() == 1 {
                repair(value, candidates[0], root, depth + 1)
            } else {
                value.clone()
            }
        }
        Some(Value::String(ty)) => match ty.as_str() {
            "number" | "integer" => coerce_number(value, ty == "integer"),
            "boolean" => match value {
                Value::String(text) if text == "true" => Value::Bool(true),
                Value::String(text) if text == "false" => Value::Bool(false),
                _ => value.clone(),
            },
            "object" => repair_object(value, schema, root, depth),
            "array" => repair_array(value, schema, root, depth),
            _ => value.clone(),
        },
        _ => value.clone(),
    }
}

/// `#/$defs/<name>` / `#/definitions/<name>` — one plain segment.
fn is_defs_ref(reference: &str, prefix: &str) -> bool {
    let Some(rest) = reference.strip_prefix(prefix) else {
        return false;
    };
    !rest.is_empty() && !rest.contains('/')
}

/// JSON-pointer escape (`~1` -> `/`, `~0` -> `~`).
fn unescape_pointer(input: &str) -> String {
    input.replace("~1", "/").replace("~0", "~")
}

/// JS `typeof` over JSON values (arrays and objects are both `"object"`).
fn js_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) | Value::Object(_) => "object",
    }
}

/// `Number(value)` coercion for number/integer schemas.
///
/// Divergence from JS `Number()`: hexadecimal and `Infinity` literals parse
/// in JS but are rejected here; both keep the original string unless the
/// result would be a non-finite/unsafe number, so observable parity holds
/// for the JSON corpus tools emit.
fn coerce_number(value: &Value, integer: bool) -> Value {
    let Value::String(text) = value else {
        return value.clone();
    };
    if text.trim().is_empty() {
        return value.clone();
    }
    let Ok(parsed) = text.trim().parse::<f64>() else {
        return value.clone();
    };
    if !parsed.is_finite() {
        return value.clone();
    }
    if integer && parsed.fract() != 0.0 {
        return value.clone();
    }
    if integer && parsed.abs() > 9_007_199_254_740_991.0 {
        // `Number.isSafeInteger` bound.
        return value.clone();
    }
    // Serialize integral results as JSON integers (JS `42`, not `42.0`).
    if parsed.fract() == 0.0 && parsed.abs() <= i64::MAX as f64 {
        Value::Number(serde_json::Number::from(parsed as i64))
    } else {
        serde_json::Number::from_f64(parsed).map_or_else(|| value.clone(), Value::Number)
    }
}

/// A finite JSON number.
fn is_number(value: &Value) -> bool {
    value.as_f64().is_some_and(f64::is_finite)
}

/// A safe JSON integer.
fn is_safe_integer(value: &Value) -> bool {
    value.as_f64().is_some_and(|number| {
        number.is_finite() && number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_991.0
    })
}

/// `repairObject` (ref tool-input-repair.ts:68-110).
fn repair_object(value: &Value, schema: &Value, root: &Value, depth: usize) -> Value {
    let parsed: Map<String, Value> = match value {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(parsed)) => parsed,
            _ => return value.clone(),
        },
        Value::Object(parsed) => parsed.clone(),
        _ => return value.clone(),
    };

    let empty = Map::new();
    let properties = schema.get("properties").and_then(Value::as_object).unwrap_or(&empty);
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let patterned = schema.get("patternProperties").is_some_and(Value::is_object);
    let composed = ["allOf", "anyOf", "oneOf"]
        .iter()
        .any(|keyword| schema.get(keyword).is_some_and(Value::is_array));

    let mut result: Map<String, Value> = parsed;
    for key in parsed.keys() {
        let current = &result[key];
        let declared = properties.contains_key(key);
        let property = if declared {
            properties.get(key)
        } else if !patterned {
            schema.get("additionalProperties")
        } else {
            None
        };

        // Undeclared keys survive unless the object is provably closed.
        if !declared
            && schema.get("additionalProperties") == Some(&Value::Bool(false))
            && !patterned
            && !composed
        {
            result.remove(key);
            continue;
        }
        let Some(property) = property.filter(|property| property.is_object()) else {
            continue;
        };

        // Only a bare single-type property provably rejects null and `{}`.
        // Compositions, enums, constants, references and nullable flags may
        // accept them, so those are left alone.
        let plain = matches!(property.get("type"), Some(Value::String(ty)) if ty != "null")
            && !composed
            && ["anyOf", "oneOf", "allOf", "enum", "const", "$ref", "nullable"]
                .iter()
                .all(|keyword| !property.as_object().is_some_and(|map| map.contains_key(keyword)));
        let placeholder = current.as_object().is_some_and(Map::is_empty)
            && property.get("type").and_then(Value::as_str) != Some("object");
        if declared && !required.contains(&key.as_str()) && plain && (current.is_null() || placeholder)
        {
            result.remove(key);
            continue;
        }

        let repaired = repair(current, property, root, depth + 1);
        if repaired != *current {
            result.insert(key.clone(), repaired);
        }
    }
    Value::Object(result)
}

/// `repairArray` (ref tool-input-repair.ts:112-152).
fn repair_array(value: &Value, schema: &Value, root: &Value, depth: usize) -> Value {
    let parsed: Value = match value {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(parsed) => parsed,
            Err(_) => return value.clone(),
        },
        parsed => parsed.clone(),
    };
    let prefix_items = schema.get("prefixItems").and_then(Value::as_array);
    let items_array = schema.get("items").and_then(Value::as_array);
    let tuple = prefix_items.or(items_array);

    if let Value::Array(items) = &parsed {
        let repaired: Vec<Value> = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let member = match tuple {
                    Some(tuple) => tuple
                        .get(index)
                        .filter(|member| !member.is_null())
                        .or_else(|| {
                            if prefix_items.is_some() {
                                schema.get("items")
                            } else {
                                schema.get("additionalItems")
                            }
                        }),
                    None => schema.get("items"),
                };
                match member.filter(|member| member.is_object()) {
                    Some(member) => repair(item, member, root, depth + 1),
                    None => item.clone(),
                }
            })
            .collect();
        if repaired.iter().zip(items.iter()).all(|(left, right)| left == right) {
            return parsed;
        }
        return Value::Array(repaired);
    }

    if tuple.is_some() || !schema.get("items").is_some_and(Value::is_object) {
        return value.clone();
    }
    let items_schema = schema.get("items").unwrap_or(&Value::Null);
    let repaired = repair(value, items_schema, root, depth + 1);
    let ty = items_schema.get("type").and_then(Value::as_str);
    let compatible = match ty {
        Some("object") => repaired.is_object(),
        Some("array") => repaired.is_array(),
        Some("integer") => is_safe_integer(&repaired),
        Some("number") => is_number(&repaired),
        Some("string" | "boolean") => js_type(&repaired) == ty,
        _ => false,
    };
    if compatible {
        Value::Array(vec![repaired])
    } else {
        value.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::{HookRegistry, HookScope};
    use crate::location::{ToolRegistryState, ToolRegistration};
    use serde_json::json;

    fn registry_with_tool(input_schema: Value) -> (Arc<HookRegistry>, Arc<ToolRegistryState>) {
        let tools = Arc::new(ToolRegistryState::new());
        tools.register(ToolRegistration {
            name: "test".into(),
            description: "Test repair".into(),
            input: input_schema,
            namespace: None,
            permission: None,
            codemode: true,
        });
        (Arc::new(HookRegistry::new()), tools)
    }

    /// Mirrors the ref harness (test/plugin/tool-input-repair.test.ts:9-37):
    /// one registered `test` tool, the plugin hook, one dispatch.
    async fn run(input: Value, input_schema: Value) -> Value {
        let (registry, tools) = registry_with_tool(input_schema);
        let scope = HookScope::new();
        scope.track(install_hook(&registry, &tools));
        let event = ToolExecuteBeforeEvent {
            tool: "test".into(),
            session_id: oc_schema::id::SessionId::new(),
            agent: oc_schema::id::AgentId::parse("build"),
            message_id: oc_schema::id::MessageId::new(),
            call_id: "call_repair".into(),
            input,
        };
        registry.dispatch_tool_execute_before(event).await.unwrap().input
    }

    fn object(properties: Value, required: Option<Vec<&str>>) -> Value {
        let mut schema = json!({ "type": "object", "properties": properties });
        if let Some(required) = required {
            schema["required"] = json!(required);
        }
        schema
    }

    // ref test 1: valid input is preserved (including extras and nested
    // containers).
    #[tokio::test]
    async fn preserves_valid_input() {
        let schema = object(
            json!({
                "count": { "type": "integer" },
                "nested": object(json!({ "enabled": { "type": "boolean" } }), None),
                "items": { "type": "array", "items": { "type": "number" } },
            }),
            None,
        );
        let input = json!({ "count": 2, "nested": { "enabled": true }, "items": [2, 3], "extra": "keep" });
        assert_eq!(run(input.clone(), schema).await, input);
    }

    // ref test 2: root stringified objects parse; nested stringified
    // containers repair recursively; broken/non-object JSON and null stay.
    #[tokio::test]
    async fn parses_stringified_containers() {
        let schema = object(
            json!({
                "count": { "type": "integer" },
                "item": object(json!({ "enabled": { "type": "boolean" } }), None),
                "list": { "type": "array", "items": { "type": "integer" } },
            }),
            None,
        );
        let repaired = run(
            json!("{\"count\":\"2\",\"item\":\"{\\\"enabled\\\":\\\"false\\\"}\",\"list\":\"[\\\"3\\\"]\"}"),
            schema.clone(),
        )
        .await;
        assert_eq!(repaired, json!({ "count": 2, "item": { "enabled": false }, "list": [3] }));
        assert_eq!(run(json!("{broken"), schema.clone()).await, json!("{broken"));
        assert_eq!(run(json!("[]"), schema.clone()).await, json!("[]"));
        assert_eq!(run(Value::Null, schema).await, Value::Null);
    }

    // ref test 3: extras drop only from explicitly closed objects.
    #[tokio::test]
    async fn removes_extras_from_closed_objects() {
        let schema = json!({
            "type": "object",
            "properties": {
                "known": { "type": "integer" },
                "closed": {
                    "type": "object",
                    "properties": { "keep": { "type": "integer" } },
                    "additionalProperties": false
                },
                "open": object(json!({ "keep": { "type": "integer" } }), None),
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "keep": { "type": "integer" } },
                        "additionalProperties": false
                    }
                }
            },
            "additionalProperties": false
        });
        let input = json!({
            "known": "2",
            "extra": true,
            "closed": { "keep": "3", "extra": true },
            "open": { "keep": "4", "extra": true },
            "items": [{ "keep": "5", "extra": true }],
        });
        assert_eq!(
            run(input, schema.clone()).await,
            json!({ "known": 2, "closed": { "keep": 3 }, "open": { "keep": 4, "extra": true }, "items": [{ "keep": 5 }] })
        );
        assert_eq!(
            run(json!({ "extra": true }), json!({ "type": "object", "properties": {}, "additionalProperties": false }))
                .await,
            json!({})
        );
    }

    // ref test 4: patterned ownership is ambiguous — keys survive.
    #[tokio::test]
    async fn preserves_unknown_keys_with_pattern_properties() {
        let schema = json!({
            "type": "object",
            "properties": { "known": { "type": "integer" } },
            "additionalProperties": false,
            "patternProperties": { "^match$": { "type": "integer" } }
        });
        let input = json!({ "known": 1, "match": "2", "extra": true });
        assert_eq!(run(input.clone(), schema).await, input);
    }

    // ref test 5: composed object schemas keep their extras.
    #[tokio::test]
    async fn preserves_properties_of_composed_schemas() {
        let input = json!({ "name": "example", "extra": true });
        for keyword in ["allOf", "anyOf", "oneOf"] {
            let mut schema = json!({
                "type": "object",
                "properties": { "name": { "type": "string" } },
                "additionalProperties": false
            });
            schema[keyword] = json!([object(json!({ "name": { "type": "string" } }), None)]);
            assert_eq!(run(input.clone(), schema).await, input, "keyword {keyword}");
        }
    }

    // ref test 6: only optional, bare-typed nulls and non-object empty
    // placeholders drop.
    #[tokio::test]
    async fn removes_only_plain_optional_nulls_and_placeholders() {
        let schema = object(
            json!({
                "optional": { "type": "string" },
                "required": { "type": "string" },
                "nullable": { "type": "string", "nullable": true },
                "union": { "anyOf": [{ "type": "integer" }, { "type": "null" }] },
                "constant": { "anyOf": [{ "type": "integer" }, { "const": null }] },
                "permissive": { "anyOf": [{ "type": "integer" }, true] },
                "referenced": { "anyOf": [{ "type": "integer" }, { "$ref": "#/$defs/nullable" }] },
                "placeholder": { "type": "integer" },
                "array": { "type": "array", "items": { "type": "string" } },
                "requiredPlaceholder": { "type": "boolean" },
                "object": { "type": "object" },
                "unknown": {}
            }),
            Some(vec!["required", "requiredPlaceholder"]),
        );
        let input = json!({
            "optional": null,
            "required": null,
            "nullable": null,
            "union": null,
            "constant": null,
            "permissive": null,
            "referenced": null,
            "placeholder": {},
            "array": {},
            "requiredPlaceholder": {},
            "object": {},
            "unknown": null,
        });
        assert_eq!(
            run(input, schema).await,
            json!({
                "required": null,
                "nullable": null,
                "union": null,
                "constant": null,
                "permissive": null,
                "referenced": null,
                "requiredPlaceholder": {},
                "object": {},
                "unknown": null,
            })
        );
    }

    // ref test 7: nulls whose validity hides inside compositions survive.
    #[tokio::test]
    async fn preserves_composition_hidden_nulls() {
        let schema = object(
            json!({
                "wrapped": { "anyOf": [{ "type": ["string", "null"] }] },
                "enumerated": { "anyOf": [{ "enum": [null, "keep"] }] },
                "permissive": { "anyOf": [{}] },
                "constant": { "type": "string", "const": "keep" }
            }),
            None,
        );
        let input = json!({ "wrapped": null, "enumerated": null, "permissive": null, "constant": null });
        assert_eq!(run(input.clone(), schema).await, input);
    }

    // ref test 8: an optional-looking null survives when the parent
    // composes requirements.
    #[tokio::test]
    async fn preserves_optional_looking_nulls_in_composed_parents() {
        let schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "allOf": [{ "required": ["value"] }]
        });
        let input = json!({ "value": null });
        assert_eq!(run(input.clone(), schema).await, input);
    }

    // ref test 9: numeric/boolean string coercion with edge preservation.
    #[tokio::test]
    async fn coerces_numeric_and_boolean_strings() {
        let schema = object(
            json!({
                "number": { "type": "number" },
                "integer": { "type": "integer" },
                "enabled": { "type": "boolean" },
                "disabled": { "type": "boolean" },
                "valid": { "type": "integer" },
                "empty": { "type": "number" },
                "infinite": { "type": "number" },
                "fractional": { "type": "integer" },
                "unsafe": { "type": "integer" },
                "uppercase": { "type": "boolean" }
            }),
            None,
        );
        let input = json!({
            "number": "1.5",
            "integer": "42",
            "enabled": "true",
            "disabled": "false",
            "valid": 3,
            "empty": " ",
            "infinite": "Infinity",
            "fractional": "1.5",
            "unsafe": "9007199254740992",
            "uppercase": "TRUE",
        });
        let mut expected = input.clone();
        expected["number"] = json!(1.5);
        expected["integer"] = json!(42);
        expected["enabled"] = json!(true);
        expected["disabled"] = json!(false);
        assert_eq!(run(input, schema).await, expected);
    }

    // ref test 10: compatible scalars wrap into single-item arrays after
    // repair.
    #[tokio::test]
    async fn wraps_compatible_scalars_into_arrays() {
        let schema = object(
            json!({
                "text": { "type": "array", "items": { "type": "string" } },
                "integer": { "type": "array", "items": { "type": "integer" } },
                "boolean": { "type": "array", "items": { "type": "boolean" } },
                "item": { "type": "array", "items": object(json!({ "count": { "type": "integer" } }), None) },
                "incompatible": { "type": "array", "items": { "type": "string" } },
                "fractional": { "type": "array", "items": { "type": "integer" } },
                "unconstrained": { "type": "array" }
            }),
            None,
        );
        assert_eq!(
            run(
                json!({
                    "text": "one",
                    "integer": "42",
                    "boolean": "false",
                    "item": "{\"count\":\"2\"}",
                    "incompatible": 2,
                    "fractional": 1.5,
                    "unconstrained": "4"
                }),
                schema
            )
            .await,
            json!({
                "text": ["one"],
                "integer": [42],
                "boolean": [false],
                "item": [{ "count": 2 }],
                "incompatible": 2,
                "fractional": 1.5,
                "unconstrained": "4"
            })
        );
    }

    // ref test 11: nested question-like inputs repair recursively.
    #[tokio::test]
    async fn repairs_nested_question_inputs() {
        let schema = object(
            json!({
                "questions": {
                    "type": "array",
                    "items": object(
                        json!({
                            "question": { "type": "string" },
                            "multiple": { "type": "boolean" },
                            "options": {
                                "type": "array",
                                "items": object(
                                    json!({ "label": { "type": "string" }, "description": { "type": "string" } }),
                                    Some(vec!["label"])
                                )
                            }
                        }),
                        None
                    )
                }
            }),
            None,
        );
        assert_eq!(
            run(
                json!({ "questions": [{ "question": "Pick one", "multiple": "false", "options": { "label": "First", "description": null } }] }),
                schema
            )
            .await,
            json!({ "questions": [{ "question": "Pick one", "multiple": false, "options": [{ "label": "First" }] }] })
        );
    }

    // ref test 12: unique nullable alternatives repair; ambiguous unions
    // and already-accepted values stay.
    #[tokio::test]
    async fn repairs_unique_nullable_alternatives() {
        let schema = object(
            json!({
                "number": { "anyOf": [{ "type": "number" }, { "type": "null" }] },
                "boolean": { "oneOf": [{ "type": "boolean" }, { "type": "null" }] },
                "nullable": { "anyOf": [{ "type": "number" }, { "type": "null" }] },
                "typed": { "type": ["integer", "null"] },
                "typedBoolean": { "type": ["boolean", "null"] },
                "accepted": { "anyOf": [{ "type": "string" }, { "type": "number" }] },
                "valid": { "type": ["number", "null"] }
            }),
            None,
        );
        let input = json!({
            "number": "2",
            "boolean": "false",
            "nullable": null,
            "typed": "3",
            "typedBoolean": "true",
            "accepted": "4",
            "valid": 5
        });
        let mut expected = input.clone();
        expected["number"] = json!(2);
        expected["boolean"] = json!(false);
        expected["typed"] = json!(3);
        expected["typedBoolean"] = json!(true);
        assert_eq!(run(input, schema).await, expected);
    }

    // ref test 13: tuple positions and rest items repair.
    #[tokio::test]
    async fn repairs_tuple_positions_and_rest_items() {
        let schema = object(
            json!({
                "prefix": {
                    "type": "array",
                    "prefixItems": [{ "type": "integer" }, { "type": "boolean" }],
                    "items": { "type": "number" }
                },
                "draft": { "type": "array", "items": [{ "type": "integer" }, { "type": "boolean" }] },
                "valid": {
                    "type": "array",
                    "prefixItems": [{ "type": "integer" }, { "type": "boolean" }]
                },
                "scalar": { "type": "array", "prefixItems": [{ "type": "integer" }] }
            }),
            None,
        );
        assert_eq!(
            run(json!({ "prefix": ["2", "false", "3"], "draft": "[\"4\",\"true\"]", "valid": [2, false], "scalar": "5" }), schema)
                .await,
            json!({ "prefix": [2, false, 3], "draft": [4, true], "valid": [2, false], "scalar": "5" })
        );
    }

    // ref test 14: typed dictionaries and straightforward local refs.
    #[tokio::test]
    async fn repairs_typed_dictionaries_and_local_refs() {
        let schema = json!({
            "type": "object",
            "properties": {
                "modern": { "$ref": "#/$defs/integer" },
                "legacy": { "$ref": "#/definitions/boolean" },
                "nested": { "$ref": "#/$defs/nested" },
                "dictionary": { "type": "object", "additionalProperties": { "$ref": "#/$defs/integer" } },
                "missing": { "$ref": "#/$defs/missing" },
                "pointer": { "$ref": "#/$defs/nested/properties/count" },
                "escaped": { "$ref": "#/$defs/a~1b~0c" }
            },
            "$defs": {
                "integer": { "type": "integer" },
                "a/b~c": { "type": "integer" },
                "nested": object(json!({ "count": { "$ref": "#/$defs/integer" } }), None)
            },
            "definitions": { "boolean": { "type": "boolean" } }
        });
        assert_eq!(
            run(
                json!({
                    "modern": "2",
                    "legacy": "false",
                    "nested": { "count": "3" },
                    "dictionary": { "first": "4" },
                    "missing": "5",
                    "pointer": "6",
                    "escaped": "7"
                }),
                schema
            )
            .await,
            json!({
                "modern": 2,
                "legacy": false,
                "nested": { "count": 3 },
                "dictionary": { "first": 4 },
                "missing": "5",
                "pointer": "6",
                "escaped": 7
            })
        );
    }

    // ref test 15: ambiguous unions, compositions and unsupported roots
    // stay untouched.
    #[tokio::test]
    async fn leaves_ambiguous_inputs_unchanged() {
        let input = json!({ "numeric": "2", "objects": { "value": "3" }, "both": "4", "composed": "5", "unknown": "6" });
        let schema = object(
            json!({
                "numeric": { "anyOf": [{ "type": "number" }, { "type": "integer" }] },
                "objects": {
                    "oneOf": [
                        object(json!({ "value": { "type": "integer" } }), Some(vec!["value"])),
                        object(json!({ "value": { "type": "number" } }), Some(vec!["value"]))
                    ]
                },
                "both": { "anyOf": [{ "type": "integer" }], "oneOf": [{ "type": "integer" }] },
                "composed": { "allOf": [{ "type": "integer" }] },
                "unknown": {}
            }),
            None,
        );
        assert_eq!(run(input.clone(), schema).await, input);
        // Non-object roots and compositions without a declared type stay.
        assert_eq!(
            run(input.clone(), json!({ "properties": { "numeric": { "type": "integer" } } })).await,
            input
        );
        assert_eq!(
            run(input.clone(), json!({ "allOf": [object(json!({ "numeric": { "type": "integer" } }), None)] }))
                .await,
            input
        );
    }

    // The plugin skips the Code Mode wrapper tool and unknown tools.
    #[tokio::test]
    async fn skips_execute_and_unknown_tools() {
        let (registry, tools) = registry_with_tool(json!({ "type": "object", "properties": {} }));
        let scope = HookScope::new();
        scope.track(install_hook(&registry, &tools));
        for tool in ["execute", "missing"] {
            let event = ToolExecuteBeforeEvent {
                tool: tool.into(),
                session_id: oc_schema::id::SessionId::new(),
                agent: oc_schema::id::AgentId::parse("build"),
                message_id: oc_schema::id::MessageId::new(),
                call_id: "call".into(),
                input: json!({ "x": "1" }),
            };
            assert_eq!(registry.dispatch_tool_execute_before(event).await.unwrap().input, json!({ "x": "1" }));
        }
        // Non-object root schemas are not repaired.
        let (registry, tools) = registry_with_tool(json!({ "type": "string" }));
        let scope = HookScope::new();
        scope.track(install_hook(&registry, &tools));
        let event = ToolExecuteBeforeEvent {
            tool: "test".into(),
            session_id: oc_schema::id::SessionId::new(),
            agent: oc_schema::id::AgentId::parse("build"),
            message_id: oc_schema::id::MessageId::new(),
            call_id: "call".into(),
            input: json!({ "x": "1" }),
        };
        assert_eq!(registry.dispatch_tool_execute_before(event).await.unwrap().input, json!({ "x": "1" }));
    }
}

