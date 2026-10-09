//! Typed view over the pinned OpenAPI document (only what the generator needs).

use serde_json::{Map, Value};

pub const METHODS: [&str; 5] = ["get", "post", "put", "delete", "patch"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParamLoc {
    Path,
    Query,
    Header,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub location: ParamLoc,
    pub required: bool,
    pub schema: Value,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Json,
    OctetStream,
    EventStream,
    None,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub content_type: ContentType,
    /// Resolved JSON schema (absent for binary / empty bodies).
    pub schema: Option<Value>,
    /// Ref name when the schema is a direct `$ref`.
    pub ref_name: Option<String>,
    /// SSE `data` schema carries `contentMediaType: application/json`
    /// (JSON-in-string double encoding).
    pub data_json_string: bool,
}

#[derive(Debug, Clone)]
pub struct Body {
    pub schema: Option<Value>,
    pub ref_name: Option<String>,
    pub binary: bool,
}

#[derive(Debug, Clone)]
pub struct Operation {
    pub method: String,
    /// Raw path template, `{param}` placeholders, `*` wildcard tail (`/api/fs/read/*`).
    pub path: String,
    pub operation_id: String,
    pub params: Vec<Param>,
    pub body: Option<Body>,
    pub success: Response,
    pub errors: Vec<Response>,
}

fn schema_ref_name(schema: &Value) -> Option<String> {
    schema.get("$ref").and_then(Value::as_str).and_then(|r| r.rsplit('/').next()).map(str::to_owned)
}

fn content_of(schemas: &Map<String, Value>, resp: &Value) -> (ContentType, Option<Value>, bool) {
    let Some(content) = resp.get("content").and_then(Value::as_object) else {
        return (ContentType::None, None, false);
    };
    if let Some(json) = content.get("application/json") {
        let schema = json.get("schema").cloned();
        return (ContentType::Json, schema, false);
    }
    if let Some(sse) = content.get("text/event-stream") {
        let mut data_json_string = false;
        if let Some(schema) = sse.get("schema") {
            data_json_string = data_schema_is_json_string(schemas, schema);
        }
        return (ContentType::EventStream, sse.get("schema").cloned(), data_json_string);
    }
    if content.contains_key("application/octet-stream") {
        return (ContentType::OctetStream, None, false);
    }
    (ContentType::None, None, false)
}

fn data_schema_is_json_string(schemas: &Map<String, Value>, schema: &Value) -> bool {
    let data = schema
        .get("properties")
        .and_then(|p| p.get("data"))
        .cloned()
        .or_else(|| Some(schema.clone()));
    let Some(data) = data else {
        return false;
    };
    // The `data` payload may be a $ref (V2EventEncoded / SessionLogItemEncoded).
    let resolved = data
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.rsplit('/').next())
        .and_then(|name| schemas.get(name))
        .cloned()
        .unwrap_or(data);
    resolved.get("contentMediaType").and_then(Value::as_str) == Some("application/json")
        && resolved.get("type").and_then(Value::as_str) == Some("string")
}

fn parse_param(p: &Value, location: ParamLoc) -> Param {
    Param {
        name: p.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
        location,
        required: p.get("required").and_then(Value::as_bool).unwrap_or(false),
        schema: p.get("schema").cloned().unwrap_or(Value::Null),
        description: p.get("description").and_then(Value::as_str).map(str::to_owned),
    }
}

/// Parses all operations from the document, sorted by (path, method).
pub fn parse_operations(spec: &Value) -> Vec<Operation> {
    let mut ops = Vec::new();
    let component_schemas = schemas(spec);
    let paths = spec.get("paths").and_then(Value::as_object).expect("spec has paths");
    for (path, item) in paths {
        let path_params: Vec<Param> = item
            .get("parameters")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().map(|p| parse_param(p, ParamLoc::Path)).collect())
            .unwrap_or_default();
        for method in METHODS {
            let Some(op) = item.get(method) else { continue };
            let operation_id =
                op.get("operationId").and_then(Value::as_str).unwrap_or_default().to_owned();
            let mut params = path_params.clone();
            if let Some(extra) = op.get("parameters").and_then(Value::as_array) {
                for p in extra {
                    let loc = match p.get("in").and_then(Value::as_str) {
                        Some("query") => ParamLoc::Query,
                        Some("header") => ParamLoc::Header,
                        _ => ParamLoc::Path,
                    };
                    let param = parse_param(p, loc);
                    if !params.iter().any(|existing| existing.name == param.name) {
                        params.push(param);
                    }
                }
            }
            params.sort_by_key(|p| (p.location, p.name.clone()));

            let body = op.get("requestBody").map(|rb| {
                let (ct, schema, _) = content_of(&component_schemas, rb);
                Body {
                    ref_name: schema.as_ref().and_then(schema_ref_name),
                    schema,
                    binary: ct == ContentType::OctetStream,
                }
            });

            let mut success = None;
            let mut errors = Vec::new();
            if let Some(responses) = op.get("responses").and_then(Value::as_object) {
                for (code, resp) in responses {
                    let Ok(status) = code.parse::<u16>() else { continue };
                    let (ct, schema, data_json_string) = content_of(&component_schemas, resp);
                    let entry = Response {
                        status,
                        content_type: ct,
                        ref_name: schema.as_ref().and_then(schema_ref_name),
                        schema,
                        data_json_string,
                    };
                    if (200..=299).contains(&status) {
                        success = Some(entry);
                    } else {
                        errors.push(entry);
                    }
                }
            }
            errors.sort_by_key(|r| r.status);
            ops.push(Operation {
                method: method.to_owned(),
                path: path.clone(),
                operation_id,
                params,
                body,
                success: success.expect("every operation declares a 2xx response"),
                errors,
            });
        }
    }
    ops.sort_by_key(|a| (a.path.clone(), a.method.clone()));
    ops
}

pub fn schemas(spec: &Value) -> Map<String, Value> {
    spec.get("components")
        .and_then(|c| c.get("schemas"))
        .and_then(Value::as_object)
        .cloned()
        .expect("spec has components.schemas")
}
