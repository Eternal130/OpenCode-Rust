//! Semantic `OpenAPI` equivalence: extract the comparison-relevant structure
//! of an `OpenAPI` document (the Phase-10 `openapi` gate) and diff two extractions.
//!
//! What "semantic" means here (per the Phase-10 lane brief):
//!
//! - **path × method sets** equal — every route registered on one side
//!   exists on the other with the same verb set;
//! - **per-operation**: `operationId`, the canonical parameter list
//!   (path-level parameters first, then operation-level, each compared as
//!   a whole JSON value), the `$ref` names reachable from parameters /
//!   request body / each response status, and the response-status set;
//! - **component schema names** equal.
//!
//! Key order inside JSON objects is not semantic (serde's default map is
//! sorted), so byte-level key ordering never produces a difference. The
//! differ returns a deterministic, human-readable list of every difference;
//! the caller asserts it is empty minus an explicit, justified whitelist
//! (`goldens/p10/openapi-whitelist.json`).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::httpdiff::deep_diff;

/// The HTTP methods an `OpenAPI` path item can carry.
const METHODS: &[&str] = &["get", "put", "post", "delete", "options", "head", "patch", "trace"];

/// One operation's comparison-relevant structure.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationSemantics {
    /// `operationId` (None when absent).
    pub operation_id: Option<String>,
    /// Canonical parameter objects (path-level first, then op-level).
    pub parameters: Vec<Value>,
    /// Sorted `$ref` component names reachable from the parameters.
    pub parameter_refs: BTreeSet<String>,
    /// Sorted `$ref` component names reachable from the request body.
    pub request_body_refs: BTreeSet<String>,
    /// Whether the request body is marked required.
    pub request_body_required: bool,
    /// Response status (or `"default"`) → sorted `$ref` names.
    pub responses: BTreeMap<String, BTreeSet<String>>,
}

/// A document's comparison-relevant structure.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocumentSemantics {
    /// path → registered methods.
    pub path_methods: BTreeMap<String, BTreeSet<String>>,
    /// (path, method) → operation structure.
    pub operations: BTreeMap<(String, String), OperationSemantics>,
    /// `components.schemas` names.
    pub schema_names: BTreeSet<String>,
}

/// Extract the comparison-relevant structure of an `OpenAPI` document.
#[must_use]
pub fn extract(doc: &Value) -> DocumentSemantics {
    let mut out = DocumentSemantics::default();
    let empty = serde_json::Map::<String, Value>::new();
    let paths = doc.get("paths").and_then(Value::as_object).unwrap_or(&empty);

    for (path, item) in paths {
        let Some(item) = item.as_object() else { continue };
        let path_parameters =
            item.get("parameters").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut methods = BTreeSet::new();
        for method in METHODS {
            if !item.contains_key(*method) {
                continue;
            }
            methods.insert((*method).to_string());
            let op = item.get(*method).and_then(Value::as_object).cloned().unwrap_or_default();
            let mut parameters = path_parameters.clone();
            parameters.extend(
                op.get("parameters").and_then(Value::as_array).cloned().unwrap_or_default(),
            );
            let parameter_refs =
                parameters.iter().chain(path_parameters.iter()).flat_map(refs_of).collect();
            let request_body =
                op.get("requestBody").and_then(Value::as_object).cloned().unwrap_or_default();
            out.operations.insert(
                (path.clone(), (*method).to_string()),
                OperationSemantics {
                    operation_id: op.get("operationId").and_then(Value::as_str).map(str::to_string),
                    parameters,
                    parameter_refs,
                    request_body_refs: refs_of(&Value::Object(request_body.clone())),
                    request_body_required: request_body
                        .get("required")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    responses: op
                        .get("responses")
                        .and_then(Value::as_object)
                        .map(|responses| {
                            responses
                                .iter()
                                .map(|(status, response)| (status.clone(), refs_of(response)))
                                .collect()
                        })
                        .unwrap_or_default(),
                },
            );
        }
        if !methods.is_empty() || !path_parameters.is_empty() {
            out.path_methods.insert(path.clone(), methods);
        }
    }

    if let Some(schemas) = doc.pointer("/components/schemas").and_then(Value::as_object) {
        out.schema_names = schemas.keys().cloned().collect();
    }
    out
}

/// Sorted, deduped `$ref` component names reachable anywhere under `value`
/// (`#/components/schemas/Foo` → `Foo`; other fragments kept verbatim).
fn refs_of(value: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect_refs(value, &mut out);
    out
}

fn collect_refs(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == "$ref" {
                    if let Some(target) = child.as_str() {
                        let name = target.strip_prefix("#/components/schemas/").unwrap_or(target);
                        out.insert(name.to_string());
                    }
                } else {
                    collect_refs(child, out);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_refs(item, out);
            }
        }
        _ => {}
    }
}

/// Deterministic list of every semantic difference (`reference` is the
/// pinned v2.0.24 document, `candidate` the served one).
#[must_use]
pub fn diff(reference: &DocumentSemantics, candidate: &DocumentSemantics) -> Vec<String> {
    let mut out = Vec::new();

    for path in reference.path_methods.keys().collect::<BTreeSet<_>>() {
        if !candidate.path_methods.contains_key(path) {
            out.push(format!("path only in reference: {path}"));
        }
    }
    for path in candidate.path_methods.keys().collect::<BTreeSet<_>>() {
        if !reference.path_methods.contains_key(path) {
            out.push(format!("path only in candidate: {path}"));
        }
    }

    for (path, ref_methods) in &reference.path_methods {
        let Some(cand_methods) = candidate.path_methods.get(path) else { continue };
        for method in ref_methods.difference(cand_methods) {
            out.push(format!("{method} {path}: method only in reference"));
        }
        for method in cand_methods.difference(ref_methods) {
            out.push(format!("{method} {path}: method only in candidate"));
        }
    }

    let mut keys: BTreeSet<&(String, String)> = reference.operations.keys().collect();
    keys.extend(candidate.operations.keys());
    for key in keys {
        let (path, method) = key;
        let Some(a) = reference.operations.get(key) else { continue };
        let Some(b) = candidate.operations.get(key) else { continue };
        let label = format!("{method} {path}");
        if a.operation_id != b.operation_id {
            out.push(format!(
                "{label}: operationId reference={:?} candidate={:?}",
                a.operation_id, b.operation_id
            ));
        }
        if a.parameters != b.parameters {
            let detail =
                deep_diff(&Value::Array(a.parameters.clone()), &Value::Array(b.parameters.clone()))
                    .map_or_else(|| "parameters differ".to_string(), |d| d.to_string());
            out.push(format!("{label}: {detail}"));
        }
        if a.parameter_refs != b.parameter_refs {
            out.push(format!(
                "{label}: parameter refs reference={:?} candidate={:?}",
                a.parameter_refs, b.parameter_refs
            ));
        }
        if a.request_body_refs != b.request_body_refs {
            out.push(format!(
                "{label}: requestBody refs reference={:?} candidate={:?}",
                a.request_body_refs, b.request_body_refs
            ));
        }
        if a.request_body_required != b.request_body_required {
            out.push(format!(
                "{label}: requestBody required reference={} candidate={}",
                a.request_body_required, b.request_body_required
            ));
        }
        let ref_statuses: BTreeSet<_> = a.responses.keys().collect();
        let cand_statuses: BTreeSet<_> = b.responses.keys().collect();
        if ref_statuses != cand_statuses {
            out.push(format!(
                "{label}: response statuses reference={ref_statuses:?} candidate={cand_statuses:?}"
            ));
        }
        for (status, ref_refs) in &a.responses {
            if let Some(cand_refs) = b.responses.get(status) {
                if ref_refs != cand_refs {
                    out.push(format!(
                        "{label}: responses.{status} refs reference={ref_refs:?} candidate={cand_refs:?}"
                    ));
                }
            }
        }
    }

    for name in reference.schema_names.difference(&candidate.schema_names) {
        out.push(format!("schema only in reference: {name}"));
    }
    for name in candidate.schema_names.difference(&reference.schema_names) {
        out.push(format!("schema only in candidate: {name}"));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc() -> Value {
        json!({
            "openapi": "3.0.0",
            "paths": {
                "/api/session": {
                    "get": {
                        "operationId": "session.list",
                        "parameters": [
                            {"name": "cursor", "in": "query", "required": false,
                             "schema": {"$ref": "#/components/schemas/Cursor"}}
                        ],
                        "responses": {
                            "200": {"content": {"application/json": {"schema":
                                {"$ref": "#/components/schemas/SessionsResponse"}}}},
                            "401": {"content": {"application/json": {"schema":
                                {"$ref": "#/components/schemas/UnauthorizedErrorEncoded"}}}}
                        }
                    },
                    "post": {
                        "operationId": "session.create",
                        "requestBody": {"required": true, "content": {"application/json": {"schema":
                            {"$ref": "#/components/schemas/SessionCreateBody"}}}},
                        "responses": {
                            "200": {"content": {"application/json": {"schema":
                                {"$ref": "#/components/schemas/SessionInfo"}}}}
                        }
                    }
                }
            },
            "components": {"schemas": {
                "Cursor": {"type": "string"},
                "SessionsResponse": {"type": "object"},
                "UnauthorizedErrorEncoded": {"type": "object"},
                "SessionCreateBody": {"type": "object"},
                "SessionInfo": {"type": "object"}
            }}
        })
    }

    #[test]
    fn identical_documents_have_no_differences() {
        assert_eq!(diff(&extract(&doc()), &extract(&doc())), Vec::<String>::new());
    }

    #[test]
    fn object_key_order_is_not_a_difference() {
        // Same document re-serialized with different key order (serde's
        // sorted map already collapses this, but pin it explicitly).
        let a = doc();
        let mut b = doc();
        b["paths"]["/api/session"]["get"]["responses"]
            .as_object_mut()
            .unwrap()
            .insert("zzz_note".into(), json!("order shuffle marker"));
        b["paths"]["/api/session"]["get"]["responses"].as_object_mut().unwrap().remove("zzz_note");
        assert_eq!(diff(&extract(&a), &extract(&b)), Vec::<String>::new());
    }

    #[test]
    fn detects_missing_path_and_method() {
        let mut b = doc();
        b["paths"].as_object_mut().unwrap().remove("/api/session");
        let d = diff(&extract(&doc()), &extract(&b));
        assert!(d.contains(&"path only in reference: /api/session".to_string()), "{d:?}");

        let mut b = doc();
        b["paths"]["/api/session"].as_object_mut().unwrap().remove("post");
        let d = diff(&extract(&doc()), &extract(&b));
        assert!(
            d.contains(&"post /api/session: method only in reference".to_string())
                || d.iter().any(|item| item.contains("method only in reference")),
            "{d:?}"
        );
    }

    #[test]
    fn detects_changed_refs_and_statuses() {
        let mut b = doc();
        b["paths"]["/api/session"]["get"]["responses"]["200"]["content"]["application/json"]
            ["schema"]["$ref"] = json!("#/components/schemas/SessionInfo");
        let d = diff(&extract(&doc()), &extract(&b));
        assert!(
            d.iter().any(
                |item| item.contains("responses.200 refs") && item.contains("SessionsResponse")
            ),
            "{d:?}"
        );

        let mut b = doc();
        b["paths"]["/api/session"]["get"]["responses"].as_object_mut().unwrap().remove("401");
        let d = diff(&extract(&doc()), &extract(&b));
        assert!(d.iter().any(|item| item.contains("response statuses")), "{d:?}");
    }

    #[test]
    fn detects_schema_and_parameter_changes() {
        let mut b = doc();
        b["components"]["schemas"].as_object_mut().unwrap().remove("Cursor");
        let d = diff(&extract(&doc()), &extract(&b));
        assert!(d.contains(&"schema only in reference: Cursor".to_string()), "{d:?}");

        let mut b = doc();
        b["paths"]["/api/session"]["get"]["parameters"][0]["name"] = json!("kursor");
        let d = diff(&extract(&doc()), &extract(&b));
        assert!(d.iter().any(|item| item.contains("divergence at $[0].name")), "{d:?}");
    }

    #[test]
    fn detects_operation_id_and_request_body_changes() {
        let mut b = doc();
        b["paths"]["/api/session"]["post"]["operationId"] = json!("session.create2");
        let d = diff(&extract(&doc()), &extract(&b));
        assert!(d.iter().any(|item| item.contains("operationId")), "{d:?}");

        let mut b = doc();
        b["paths"]["/api/session"]["post"]["requestBody"]["required"] = json!(false);
        let d = diff(&extract(&doc()), &extract(&b));
        assert!(d.iter().any(|item| item.contains("requestBody required")), "{d:?}");
    }
}
