//! Tool schema projection (port of `src/protocols/utils/tool-schema.ts` —
//! `ToolSchemaProjection`): JSON-Schema → provider-compatible tool schemas.
//!
//! Tool arguments are always a JSON object, and most providers reject a tool
//! schema whose root does not declare `type: "object"` ([`object_root`]
//! fixes that for every tool). On top of the object root, an explicit
//! sanitizer (model compatibility → protocol default → model-name family)
//! rewrites shapes a provider family rejects: Gemini
//! ([`gemini_json_schema`]) and Moonshot ([`moonshot`]). `none` opts out.
//!
//! In TS this runs once per request inside `prepareRequest` before any
//! protocol builds its body; the Rust base route exposes the plumbing
//! (`Route::sanitizer` + `SanitizerCompatibility`) and protocol body
//! builders apply the projection to their flattened tool list — the lowered
//! bodies are identical because the projection only rewrites leaf
//! `inputSchema`s.

use serde_json::{Map, Value};

use crate::schema::{LanguageModel, SanitizerCompatibility, ToolEntry};

use super::utils::gemini_json_schema;

/// Keys whose values are plain value lists, never schema maps — recursing
/// into them would corrupt the schema.
pub(crate) const VALUES: [&str; 5] = ["const", "default", "enum", "examples", "dependentRequired"];

/// The Gemini projection: the shared JSON-Schema normalization.
#[must_use]
pub fn gemini(schema: &Value) -> Value {
    gemini_json_schema::normalize(schema)
}

/// `items` schema for a Moonshot tuple: one member → itself, several →
/// `anyOf`, none → the unconstrained schema.
fn tuple_items_schema(items: &[Value]) -> Value {
    let projected: Vec<Value> = items.iter().map(moonshot_node).collect();
    match projected.len() {
        0 => Value::Object(Map::new()),
        1 => projected.into_iter().next().unwrap_or(Value::Null),
        _ => Value::Object(Map::from_iter([("anyOf".to_string(), Value::Array(projected))])),
    }
}

/// Moonshot rejects an `enum` without a sibling `type`. Its `type` may be
/// one type, or one type plus `"null"`.
fn enum_type(values: &[Value]) -> Option<Value> {
    let mut types: Vec<&str> = Vec::new();
    for value in values {
        let kind = match value {
            Value::Null => "null",
            Value::Array(_) => "array",
            Value::String(_) => "string",
            Value::Number(_) => "number",
            Value::Bool(_) => "boolean",
            Value::Object(_) => "object",
        };
        if !types.contains(&kind) {
            types.push(kind);
        }
    }
    match types.len() {
        1 => Some(Value::String(types[0].to_string())),
        2 if types.contains(&"null") => {
            let other = types.into_iter().find(|kind| *kind != "null").unwrap_or("null");
            Some(Value::Array(vec![
                Value::String(other.to_string()),
                Value::String("null".to_string()),
            ]))
        }
        _ => None,
    }
}

/// Moonshot rejects `$ref` siblings and 2020-12 tuple keywords: a `$ref`
/// node collapses to `{$ref}`, `prefixItems`/array-`items` become a single
/// `items` schema object, `unevaluatedItems` is dropped, and untyped enums
/// gain a derived `type`.
fn moonshot_node(schema: &Value) -> Value {
    match schema {
        Value::Array(items) => Value::Array(items.iter().map(moonshot_node).collect()),
        Value::Object(record) => {
            if let Some(reference) = record.get("$ref").and_then(Value::as_str) {
                return Value::Object(Map::from_iter([(
                    "$ref".to_string(),
                    Value::String(reference.to_string()),
                )]));
            }
            let derived_type = if record.contains_key("type") {
                None
            } else {
                record.get("enum").and_then(Value::as_array).and_then(|values| enum_type(values))
            };
            let mut out = Map::new();
            if let Some(kind) = derived_type {
                out.insert("type".to_string(), kind);
            }
            for (key, value) in record {
                if key == "items" {
                    if let Some(items) = value.as_array() {
                        out.insert(key.clone(), tuple_items_schema(items));
                        continue;
                    }
                    out.insert(key.clone(), moonshot_node(value));
                    continue;
                }
                if key == "prefixItems" {
                    if record.contains_key("items") {
                        continue;
                    }
                    let items: &[Value] = value.as_array().map_or(&[], Vec::as_slice);
                    out.insert("items".to_string(), tuple_items_schema(items));
                    continue;
                }
                if key == "unevaluatedItems" {
                    continue;
                }
                out.insert(key.clone(), moonshot_node(value));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// The Moonshot projection.
#[must_use]
pub fn moonshot(schema: &Value) -> Value {
    let projected = moonshot_node(schema);
    if projected.is_object() {
        projected
    } else {
        Value::Object(Map::new())
    }
}

/// Declare every tool schema root as an object. Tool arguments are always a
/// JSON object and most providers reject a root without `type: "object"`
/// (such as `{}` or a bare `properties` map). An `anyOf` with a bare-object
/// branch already matches every object, so the union is dropped instead of
/// duplicated.
#[must_use]
pub fn object_root(schema: Value) -> Value {
    let Some(record) = schema.as_object() else {
        return Value::Object(Map::from_iter([(
            "type".to_string(),
            Value::String("object".to_string()),
        )]));
    };
    if record.contains_key("type") {
        return schema;
    }
    let bare_object_branch =
        record.get("anyOf").and_then(Value::as_array).is_some_and(|branches| {
            branches.iter().any(|branch| {
                branch.as_object().is_some_and(|branch| {
                    branch.len() == 1
                        && branch.get("type").and_then(Value::as_str) == Some("object")
                })
            })
        });
    let mut out = Map::new();
    out.insert("type".to_string(), Value::String("object".to_string()));
    if bare_object_branch {
        for (key, value) in record {
            if key != "anyOf" {
                out.insert(key.clone(), value.clone());
            }
        }
    } else {
        out.extend(record.iter().map(|(key, value)| (key.clone(), value.clone())));
    }
    Value::Object(out)
}

/// Resolve the sanitizer for a model: explicit compatibility wins, then the
/// protocol default carried by the route, then the model-name family (so
/// models reached through gateways and OpenAI-compatible endpoints get the
/// same handling). `None` opts out.
#[must_use]
pub fn resolve(model: &LanguageModel) -> Option<SanitizerCompatibility> {
    if let Some(compatibility) =
        model.compatibility.as_ref().and_then(|compatibility| compatibility.sanitizer)
    {
        return Some(compatibility);
    }
    if let Some(route_default) = model.route.sanitizer() {
        return Some(route_default);
    }
    let id = model.id.as_str();
    if id.to_ascii_lowercase().contains("gemini") {
        return Some(SanitizerCompatibility::Gemini);
    }
    if id.to_ascii_lowercase().contains("kimi") {
        return Some(SanitizerCompatibility::Moonshot);
    }
    None
}

/// Apply the resolved sanitizer to one (already object-rooted) schema.
#[must_use]
pub fn model_compatibility(schema: Value, model: &LanguageModel) -> Value {
    let root = object_root(schema);
    match resolve(model) {
        Some(SanitizerCompatibility::Gemini) => gemini(&root),
        Some(SanitizerCompatibility::Moonshot) => moonshot(&root),
        Some(SanitizerCompatibility::None) | None => root,
    }
}

/// Project every tool schema in a request's tool list, recursing into
/// namespaces (port of `ToolSchemaProjection.tools`).
#[must_use]
pub fn project_tools(tools: Vec<ToolEntry>, model: &LanguageModel) -> Vec<ToolEntry> {
    tools
        .into_iter()
        .map(|tool| match tool {
            ToolEntry::Definition(mut definition) => {
                definition.input_schema = model_compatibility(definition.input_schema, model);
                ToolEntry::Definition(definition)
            }
            ToolEntry::Namespace(mut namespace) => {
                namespace.tools = project_tools(namespace.tools, model);
                ToolEntry::Namespace(namespace)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn moonshot_strips_ref_siblings_and_converts_tuple_arrays() {
        let input = json!({
            "type": "object",
            "properties": {
                "linked": { "$ref": "#/$defs/Linked", "description": "drop me" },
                "tuple": { "type": "array", "items": [{ "type": "string" }, { "type": "number" }] },
                "prefixTuple": { "type": "array", "prefixItems": [{ "type": "boolean" }, { "type": "string" }] }
            }
        });
        assert_eq!(
            moonshot(&input),
            json!({
                "type": "object",
                "properties": {
                    "linked": { "$ref": "#/$defs/Linked" },
                    "tuple": { "type": "array", "items": { "anyOf": [{ "type": "string" }, { "type": "number" }] } },
                    "prefixTuple": { "type": "array", "items": { "anyOf": [{ "type": "boolean" }, { "type": "string" }] } }
                }
            })
        );
    }

    #[test]
    fn moonshot_derives_a_type_for_untyped_enums() {
        let input = json!({
            "type": "object",
            "properties": {
                "kind": { "description": "The kind of flag", "enum": ["boolean", "string"] },
                "level": { "enum": [1, 2.5] },
                "optional": { "enum": [null, "a"] },
                "choice": { "anyOf": [{ "enum": [true, false] }, { "type": "null" }] },
                "list": { "type": "array", "items": { "enum": ["x"] } },
                "map": { "type": "object", "additionalProperties": { "enum": ["y"] } },
                "typed": { "type": "string", "enum": ["a", null] },
                "mixed": { "enum": ["a", 1] }
            },
            "$defs": { "Mode": { "enum": ["fast"] } }
        });
        assert_eq!(
            moonshot(&input),
            json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "description": "The kind of flag", "enum": ["boolean", "string"] },
                    "level": { "type": "number", "enum": [1, 2.5] },
                    "optional": { "type": ["string", "null"], "enum": [null, "a"] },
                    "choice": { "anyOf": [{ "type": "boolean", "enum": [true, false] }, { "type": "null" }] },
                    "list": { "type": "array", "items": { "type": "string", "enum": ["x"] } },
                    "map": { "type": "object", "additionalProperties": { "type": "string", "enum": ["y"] } },
                    "typed": { "type": "string", "enum": ["a", null] },
                    "mixed": { "enum": ["a", 1] }
                },
                "$defs": { "Mode": { "type": "string", "enum": ["fast"] } }
            })
        );
    }

    #[test]
    fn object_roots_declare_every_schema_root_as_an_object() {
        let union = json!({
            "anyOf": [
                { "type": "object", "properties": { "a": { "type": "string" } } },
                { "type": "object", "properties": { "b": { "type": "string" } } }
            ]
        });
        assert_eq!(object_root(json!({})), json!({ "type": "object" }));
        assert_eq!(
            object_root(
                json!({ "description": "Query", "properties": { "q": { "type": "string" } } })
            ),
            json!({ "type": "object", "description": "Query", "properties": { "q": { "type": "string" } } })
        );
        // neither branch is a bare object, so the union is kept
        assert_eq!(
            object_root(union.clone()),
            json!({ "type": "object", "anyOf": union["anyOf"] })
        );
        let exclusive =
            json!({ "oneOf": [{ "type": "object" }, { "type": "object", "required": ["a"] }] });
        assert_eq!(
            object_root(exclusive.clone()),
            json!({ "type": "object", "oneOf": exclusive["oneOf"] })
        );
        let typed = json!({ "type": "object", "properties": {} });
        assert_eq!(object_root(typed.clone()), typed);
    }

    #[test]
    fn union_with_a_bare_object_branch_drops_the_anyof() {
        // every object matches the bare branch, so the union adds nothing
        let input = json!({
            "anyOf": [
                { "type": "object", "properties": { "a": { "type": "string" } } },
                { "type": "object" }
            ]
        });
        assert_eq!(object_root(input), json!({ "type": "object" }));
    }

    #[test]
    fn project_tools_applies_the_resolved_sanitizer_per_tool() {
        use crate::schema::LanguageModelCompatibility;
        use crate::test_support::echo_route_named;

        let route = echo_route_named("openai-chat");
        let original = json!({
            "type": "object",
            "required": ["mode", "missing"],
            "properties": { "mode": { "enum": ["fast", "safe"] } }
        });
        let tool = |model: crate::schema::LanguageModel| {
            let entries = project_tools(
                vec![ToolEntry::Definition(crate::schema::ToolDefinition::new(
                    "lookup",
                    "Lookup data.",
                    original.clone(),
                ))],
                &model,
            );
            match &entries[0] {
                ToolEntry::Definition(definition) => definition.input_schema.clone(),
                ToolEntry::Namespace(_) => unreachable!(),
            }
        };

        // model-name families select the projection without explicit config
        let by_name =
            crate::schema::LanguageModel::new("google/Gemini-3.8-Flash", "openai", route.clone());
        assert_eq!(
            tool(by_name),
            json!({ "type": "object", "required": ["mode"], "properties": { "mode": { "enum": ["fast", "safe"] } } })
        );
        let kimi = crate::schema::LanguageModel::new("moonshotai/Kimi-K3", "openai", route.clone());
        assert_eq!(
            tool(kimi),
            json!({
                "type": "object",
                "required": ["mode", "missing"],
                "properties": { "mode": { "type": "string", "enum": ["fast", "safe"] } }
            })
        );
        // explicit compatibility overrides the model name
        let explicit =
            crate::schema::LanguageModel::new("google/gemini-3.8-flash", "openai", route.clone())
                .with_compatibility(LanguageModelCompatibility {
                    sanitizer: Some(SanitizerCompatibility::Moonshot),
                    ..Default::default()
                });
        assert_eq!(
            tool(explicit),
            json!({
                "type": "object",
                "required": ["mode", "missing"],
                "properties": { "mode": { "type": "string", "enum": ["fast", "safe"] } }
            })
        );
        let opted_out =
            crate::schema::LanguageModel::new("google/gemini-3.8-flash", "openai", route.clone())
                .with_compatibility(LanguageModelCompatibility {
                    sanitizer: Some(SanitizerCompatibility::None),
                    ..Default::default()
                });
        assert_eq!(tool(opted_out), original);
        // unknown names without a route default keep the plain object root
        let neutral = crate::schema::LanguageModel::new("gpt-6-luna", "openai", route.clone());
        assert_eq!(tool(neutral), original);
    }

    #[test]
    fn project_tools_recurses_into_namespaces() {
        use crate::schema::LanguageModelCompatibility;
        use crate::test_support::echo_route_named;

        let route = echo_route_named("test");
        let model = crate::schema::LanguageModel::new("kimi-k2", "test", route).with_compatibility(
            LanguageModelCompatibility {
                sanitizer: Some(SanitizerCompatibility::Moonshot),
                ..Default::default()
            },
        );
        let entries = vec![ToolEntry::Namespace(crate::schema::ToolNamespace {
            name: "search".into(),
            description: None,
            tools: vec![ToolEntry::Definition(crate::schema::ToolDefinition::new(
                "web",
                "Web search",
                json!({ "type": "object", "properties": { "mode": { "enum": ["fast"] } } }),
            ))],
        })];
        let projected = project_tools(entries, &model);
        match &projected[0] {
            ToolEntry::Namespace(namespace) => match &namespace.tools[0] {
                ToolEntry::Definition(definition) => {
                    assert_eq!(
                        definition.input_schema,
                        json!({
                            "type": "object",
                            "properties": { "mode": { "type": "string", "enum": ["fast"] } }
                        })
                    );
                }
                ToolEntry::Namespace(_) => unreachable!(),
            },
            ToolEntry::Definition(_) => unreachable!(),
        }
    }
}
