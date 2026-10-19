//! HTTP-H0 request-validation seam: interprets the GENERATED constraint
//! table (`validation.gen.rs`) at the v2 `SchemaErrorMiddleware` position.
//!
//! v2 (`packages/protocol/src/middleware/schema-error.ts`) decodes every
//! request against the endpoint's Effect schema before the handler runs;
//! decode failures surface as `400 InvalidRequestError {message, kind}`.
//! The messages below are Effect `TreeFormatter` shapes, pinned
//! field-by-field against a live v2.0.24 reference boot (evidence:
//! `/tmp/opencode/parity-evidence/HTTP-H0/ref-validation-messages*.json`
//! + the Phase-10 P10-E1/E2 battery):
//!
//! ```text
//! Expected object                                (non-object payload root)
//! Expected string\n  at ["command"]              (plain field type)
//! Expected string | null\n  at ["title"]         (anyOf [X, null] field)
//! Expected Session.Inbox.Delivery | null\n  at ["delivery"]  ($ref label)
//! Expected string\n  at ["model"]["providerID"]  (nested member path)
//! Expected string\n  at ["targets"][0]           (array item index)
//! Missing key\n  at ["targets"]                  (absent required field)
//! Expected array\n  at ["targets"]
//! Expected number\n  at ["timeout"]              (integer field, wrong kind)
//! Expected an integer\n  at ["timeout"]           (fractional)
//! Expected a value greater than or equal to 0\n  at ["timeout"]
//! ```
//!
//! **Seam position** (probe-verified): v2 endpoints carrying
//! `sessionLocationMiddleware` resolve the session (404) BEFORE payload
//! decode — `session.update`, `switchAgent`, `switchModel`, `prompt`,
//! `shell`, … For those routes this middleware MUST NOT fire; their
//! post-session validation is a follow-up slice (see ledger HTTP-H0).
//! [`ENFORCED_OPS`] lists only validate-first routes (no location
//! middleware in `protocol/src/groups/*`), where v2 rejects wrong payloads
//! regardless of resource existence.
//!
//! Unverified label note: `Bool` renders `boolean` by analogy (no
//! validate-first body in the slice carries a boolean field; probe list
//! kept in the evidence file).

use axum::body::Body;
use axum::extract::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::validation_gen::{self, SchemaNode};
use serde_json::Value;

/// Body buffering limit: axum's default `DefaultBodyLimit` (2 MiB) — the
/// same budget the handlers' `Json` extractors apply after pass-through.
const BODY_LIMIT: usize = 2 * 1024 * 1024;

/// The HTTP-H0 first slice: validate-first operations whose JSON payloads
/// are enforced in middleware (sorted; mirrors the generated table). This
/// is the incremental rollout valve — it grows to the full
/// validate-first set after oracle review, and LOC-MW routes get their
/// post-session seam in the follow-up lane.
const ENFORCED_OPS: &[&str] = &[
    "credential.update",
    "plugin.update",
    "project.update",
    "pty.create",
    "pty.update",
    "session.create",
    "session.move",
    "shell.create",
    "websearch.query",
];

// ---------------------------------------------------------------------------
// Interpreter over the generated arena
// ---------------------------------------------------------------------------

/// One path segment of the failing value (`["field"]["member"][0]`).
#[derive(Debug, Clone, PartialEq)]
enum Seg {
    Key(String),
    Index(usize),
}

impl Seg {
    fn render(&self) -> String {
        match self {
            Seg::Key(key) => format!("[{key:?}]"),
            Seg::Index(index) => format!("[{index}]"),
        }
    }
}

/// A validation failure: the `Expected …` / `Missing key` line plus where.
struct Reject {
    message: String,
    path: Vec<Seg>,
}

impl Reject {
    fn expected(label: &str, path: &[Seg]) -> Self {
        Self { message: format!("Expected {label}"), path: path.to_vec() }
    }

    /// The full Effect message: message line + two-space-indented path
    /// line (no path line at the payload root).
    fn render(&self) -> String {
        if self.path.is_empty() {
            self.message.clone()
        } else {
            let path: String = self.path.iter().map(Seg::render).collect();
            format!("{}\n  at {}", self.message, path)
        }
    }
}

/// Renders a numeric bound the way Effect prints it (integral values
/// without a decimal point).
#[allow(clippy::cast_possible_truncation)] // bounds come from JSON numbers (small)
fn fmt_bound(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn check(node: u32, value: &Value, path: &mut Vec<Seg>) -> Result<(), Reject> {
    match &validation_gen::NODES[node as usize] {
        SchemaNode::Any => Ok(()),
        SchemaNode::Str => {
            if value.is_string() {
                Ok(())
            } else {
                Err(Reject::expected("string", path))
            }
        }
        SchemaNode::Bool => {
            // Label by analogy — see module docs (no slice probe yet).
            if value.is_boolean() {
                Ok(())
            } else {
                Err(Reject::expected("boolean", path))
            }
        }
        SchemaNode::Num => {
            if value.is_number() {
                Ok(())
            } else {
                Err(Reject::expected("number", path))
            }
        }
        SchemaNode::Int { minimum, exclusive_minimum } => {
            check_int(value, *minimum, *exclusive_minimum, path)
        }
        SchemaNode::Literal(values) => check_literal(values, value, path),
        SchemaNode::Nullable(inner) => {
            if value.is_null() {
                return Ok(());
            }
            check(*inner, value, path).map_err(|reject| {
                if reject.path.len() == path.len() {
                    // The nullable node itself mismatched: Effect appends
                    // `| null` to the inner expectation.
                    Reject { message: format!("{} | null", reject.message), path: reject.path }
                } else {
                    reject
                }
            })
        }
        SchemaNode::Ref { label, node } => check(*node, value, path).map_err(|reject| {
            if reject.path.len() == path.len() {
                // Whole-field mismatch: print the annotated identifier.
                Reject { message: format!("Expected {label}"), path: reject.path }
            } else {
                reject
            }
        }),
        SchemaNode::Arr(items) => {
            let Some(elements) = value.as_array() else {
                return Err(Reject::expected("array", path));
            };
            for (index, element) in elements.iter().enumerate() {
                path.push(Seg::Index(index));
                let result = check(*items, element, path);
                path.pop();
                result?;
            }
            Ok(())
        }
        SchemaNode::Record(value_node) => {
            let Some(members) = value.as_object() else {
                return Err(Reject::expected("object", path));
            };
            for (key, member) in members {
                path.push(Seg::Key(key.clone()));
                let result = check(*value_node, member, path);
                path.pop();
                result?;
            }
            Ok(())
        }
        SchemaNode::Obj { fields } => check_object(fields, value, path),
    }
}

/// `SchemaNode::Int`: JSON number, integral, within bounds (probe-pinned
/// messages: `number` → `an integer` → `a value greater than [or equal
/// to] N`).
fn check_int(
    value: &Value,
    minimum: Option<f64>,
    exclusive_minimum: Option<f64>,
    path: &[Seg],
) -> Result<(), Reject> {
    let Some(number) = value.as_f64() else {
        return Err(Reject::expected("number", path));
    };
    if !number.is_finite() || number.fract() != 0.0 {
        return Err(Reject { message: "Expected an integer".to_owned(), path: path.to_vec() });
    }
    if let Some(bound) = exclusive_minimum {
        if number <= bound {
            return Err(Reject {
                message: format!("Expected a value greater than {}", fmt_bound(bound)),
                path: path.to_vec(),
            });
        }
    }
    if let Some(bound) = minimum {
        if number < bound {
            return Err(Reject {
                message: format!("Expected a value greater than or equal to {}", fmt_bound(bound)),
                path: path.to_vec(),
            });
        }
    }
    Ok(())
}

/// `SchemaNode::Literal`: inline string-literal union
/// (`Expected "a" | "b"`).
fn check_literal(values: &[&str], value: &Value, path: &[Seg]) -> Result<(), Reject> {
    match value.as_str() {
        Some(text) if values.contains(&text) => Ok(()),
        _ => Err(Reject {
            message: format!(
                "Expected {}",
                values.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(" | ")
            ),
            path: path.to_vec(),
        }),
    }
}

/// `SchemaNode::Obj`: declaration-order walk — Effect reports the FIRST
/// offending field of the struct, in schema declaration order; missing
/// required members report `Missing key` at the member path.
fn check_object(
    fields: &[crate::validation_gen::FieldSpec],
    value: &Value,
    path: &mut Vec<Seg>,
) -> Result<(), Reject> {
    let Some(object) = value.as_object() else {
        return Err(Reject::expected("object", path));
    };
    for field in fields {
        match object.get(field.name) {
            None => {
                if field.required {
                    let mut reject_path = path.clone();
                    reject_path.push(Seg::Key(field.name.to_owned()));
                    return Err(Reject { message: "Missing key".to_owned(), path: reject_path });
                }
            }
            Some(member) => {
                path.push(Seg::Key(field.name.to_owned()));
                let result = check(field.ty, member, path);
                path.pop();
                result?;
            }
        }
    }
    Ok(())
}

/// Validates one operation's JSON payload against the generated table.
/// `Err(message)` carries the byte-pinned Effect message for the first
/// offending field.
pub fn validate_payload(operation_id: &str, body: &Value) -> Result<(), String> {
    let Some(node) = validation_gen::body_node(operation_id) else {
        return Ok(());
    };
    check(node, body, &mut Vec::new()).map_err(|reject| reject.render())
}

// ---------------------------------------------------------------------------
// Middleware (the v2 SchemaErrorMiddleware position)
// ---------------------------------------------------------------------------

/// Whether the middleware enforces this operation (validate-first slice).
#[must_use]
pub fn enforces(operation_id: &str) -> bool {
    ENFORCED_OPS.binary_search(&operation_id).is_ok()
}

fn table_method(method: &axum::http::Method) -> Option<crate::routes_gen::Method> {
    match *method {
        axum::http::Method::GET => Some(crate::routes_gen::Method::Get),
        axum::http::Method::POST => Some(crate::routes_gen::Method::Post),
        axum::http::Method::PUT => Some(crate::routes_gen::Method::Put),
        axum::http::Method::DELETE => Some(crate::routes_gen::Method::Delete),
        axum::http::Method::PATCH => Some(crate::routes_gen::Method::Patch),
        _ => None,
    }
}

/// The schema-error middleware: for enforced operations with a JSON body,
/// buffer + parse + validate the payload before the handler; violations
/// answer `400 InvalidRequestError {message, kind: "Payload"}` (the v2
/// envelope). Everything else passes through byte-identically — including
/// non-JSON bodies (P10-N1 stays with the extractor path) and
/// resource-first routes (404 must precede any payload 400 there).
pub async fn schema_error_middleware(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let operation_id = table_method(&method)
        .and_then(|table_method| {
            crate::routes_gen::find(table_method, &path).map(|(route, _)| route.operation_id)
        })
        .filter(|operation_id| enforces(operation_id));
    let json_content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    let Some(operation_id) = operation_id.filter(|_| json_content_type) else {
        return next.run(request).await;
    };

    let (parts, body) = request.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, BODY_LIMIT).await else {
        // Over the same 2 MiB budget the `Json` extractor would have
        // applied; mirror its plain-text 413.
        return (axum::http::StatusCode::PAYLOAD_TOO_LARGE, "length limit exceeded")
            .into_response();
    };
    let pass_through = |bytes: axum::body::Bytes| Request::from_parts(parts, Body::from(bytes));
    match serde_json::from_slice::<Value>(&bytes) {
        // Unparseable JSON: hand the buffered bytes to the handler and let
        // the `Json` extractor answer (P10-N1 divergence, unchanged here).
        Err(_) => next.run(pass_through(bytes)).await,
        Ok(value) => match validate_payload(operation_id, &value) {
            Ok(()) => next.run(pass_through(bytes)).await,
            Err(message) => crate::errors::schema_rejection(&message, "Payload").into_response(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[allow(clippy::needless_pass_by_value)] // test ergonomics: json! by value
    fn err(op: &str, body: Value) -> String {
        validate_payload(op, &body).unwrap_err()
    }

    /// Every message below is byte-pinned against the live v2.0.24
    /// reference (probe evidence ref-validation-messages*.json).
    #[allow(clippy::too_many_lines)] // one pinned table, one test
    #[test]
    fn pinned_effect_messages() {
        // session.create (P10-E1 battery + probes)
        assert_eq!(
            err("session.create", json!({"title": 12345})),
            "Expected string | null\n  at [\"title\"]"
        );
        assert_eq!(err("session.create", json!([1, 2])), "Expected object");
        assert_eq!(
            err("session.create", json!({"metadata": 5})),
            "Expected Session.Metadata | null\n  at [\"metadata\"]"
        );
        assert_eq!(
            err("session.create", json!({"permissions": {}})),
            "Expected Permission.Ruleset | null\n  at [\"permissions\"]"
        );
        assert_eq!(
            err("session.create", json!({"model": {"id": "x", "providerID": 5}})),
            "Expected string\n  at [\"model\"][\"providerID\"]"
        );
        // Nested missing keys report the first missing member (probe r3).
        assert_eq!(
            err("session.create", json!({"model": {"providerID": 5}})),
            "Missing key\n  at [\"model\"][\"id\"]"
        );
        assert_eq!(
            err("session.create", json!({"model": {}})),
            "Missing key\n  at [\"model\"][\"id\"]"
        );
        // declaration order wins (id, parentID, title, …)
        assert_eq!(
            err("session.create", json!({"title": 1, "parentID": 2})),
            "Expected string | null\n  at [\"parentID\"]"
        );

        // plugin.update (required array-of-string)
        assert_eq!(
            err("plugin.update", json!({"targets": "notanarray"})),
            "Expected array\n  at [\"targets\"]"
        );
        assert_eq!(err("plugin.update", json!({})), "Missing key\n  at [\"targets\"]");
        assert_eq!(
            err("plugin.update", json!({"targets": [123]})),
            "Expected string\n  at [\"targets\"][0]"
        );

        // session.move (required string + nullable $ref enum)
        assert_eq!(err("session.move", json!({})), "Missing key\n  at [\"directory\"]");
        assert_eq!(
            err("session.move", json!({"delivery": 5})),
            "Missing key\n  at [\"directory\"]"
        );
        assert_eq!(
            err("session.move", json!({"directory": "/tmp", "delivery": 5})),
            "Expected Session.Inbox.Delivery | null\n  at [\"delivery\"]"
        );
        assert_eq!(
            err("session.move", json!({"directory": "/tmp", "delivery": "bogus"})),
            "Expected Session.Inbox.Delivery | null\n  at [\"delivery\"]"
        );

        // credential.update / websearch.query / project.update
        assert_eq!(
            err("credential.update", json!({"label": 5})),
            "Expected string\n  at [\"label\"]"
        );
        assert_eq!(
            err("websearch.query", json!({"query": "q", "providerID": 5})),
            "Expected string\n  at [\"providerID\"]"
        );
        assert_eq!(err("project.update", json!({"name": 5})), "Expected string\n  at [\"name\"]");
        assert_eq!(
            err("project.update", json!({"commands": 5})),
            "Expected Project.Commands\n  at [\"commands\"]"
        );

        // shell.create (required string, integer bounds, record)
        assert_eq!(
            err("shell.create", json!({"command": 5})),
            "Expected string\n  at [\"command\"]"
        );
        assert_eq!(
            err("shell.create", json!({"command": "ls", "timeout": "abc"})),
            "Expected number\n  at [\"timeout\"]"
        );
        assert_eq!(
            err("shell.create", json!({"command": "ls", "timeout": 1.5})),
            "Expected an integer\n  at [\"timeout\"]"
        );
        assert_eq!(
            err("shell.create", json!({"command": "ls", "timeout": -1})),
            "Expected a value greater than or equal to 0\n  at [\"timeout\"]"
        );
        assert_eq!(
            err("shell.create", json!({"command": "ls", "metadata": 5})),
            "Expected object\n  at [\"metadata\"]"
        );

        // pty.create / pty.update (arrays, nested inline struct)
        assert_eq!(
            err("pty.create", json!({"command": "sh", "args": "x"})),
            "Expected array\n  at [\"args\"]"
        );
        assert_eq!(
            err("pty.create", json!({"command": "sh", "args": [5]})),
            "Expected string\n  at [\"args\"][0]"
        );
        assert_eq!(
            err("pty.create", json!({"command": "sh", "env": 5})),
            "Expected object\n  at [\"env\"]"
        );
        assert_eq!(err("pty.update", json!({"title": 5})), "Expected string\n  at [\"title\"]");
        assert_eq!(
            err("pty.update", json!({"title": "t", "size": 5})),
            "Expected object\n  at [\"size\"]"
        );
        assert_eq!(
            err("pty.update", json!({"title": "t", "size": {"cols": "x", "rows": 2}})),
            "Expected number\n  at [\"size\"][\"cols\"]"
        );
        assert_eq!(
            err("pty.update", json!({"title": "t", "size": {"cols": 1.5, "rows": 2}})),
            "Expected an integer\n  at [\"size\"][\"cols\"]"
        );
        // Nested missing key inside an inline struct (probe r3: rows first).
        assert_eq!(
            err("pty.update", json!({"title": "t", "size": {"cols": 1}})),
            "Missing key\n  at [\"size\"][\"rows\"]"
        );

        // Record value paths (probe r3).
        assert_eq!(
            err("pty.create", json!({"command": "sh", "env": {"K": 5}})),
            "Expected string\n  at [\"env\"][\"K\"]"
        );
        // $ref'd non-null label (probe r3: Project.Icon).
        assert_eq!(
            err("project.update", json!({"icon": 5})),
            "Expected Project.Icon\n  at [\"icon\"]"
        );
        // Later array items report their own index (probe r3).
        assert_eq!(
            err("plugin.update", json!({"targets": ["ok", 5]})),
            "Expected string\n  at [\"targets\"][1]"
        );
    }

    /// Effect strips unknown fields (additionalProperties: false is NOT a
    /// rejection) and nulls pass nullable fields only.
    #[test]
    fn accepts_valid_and_strips_unknown() {
        for (op, body) in [
            ("session.create", json!({"totallyBogus": "x"})),
            ("session.create", json!({"title": null})),
            (
                "session.create",
                json!({
                    "title": "t",
                    "model": {"providerID": "p", "id": "m", "variant": "default"},
                    "metadata": {"k": [1, {"nested": true}]},
                    "permissions": [{"action": "*", "resource": "*", "effect": "allow"}],
                }),
            ),
            ("plugin.update", json!({"targets": ["a", "b"]})),
            ("session.move", json!({"directory": "/tmp"})),
            ("session.move", json!({"directory": "/tmp", "delivery": "steer"})),
            ("session.move", json!({"directory": "/tmp", "delivery": null})),
            ("shell.create", json!({"command": "ls", "timeout": 5, "metadata": {"k": 1}})),
            ("pty.update", json!({"title": "t", "size": {"cols": 2, "rows": 1}})),
            ("websearch.query", json!({"query": "q"})),
        ] {
            assert!(validate_payload(op, &body).is_ok(), "{op} rejected valid payload {body}");
        }
        // Non-nullable fields still reject explicit nulls (probe:
        // shell.create timeout:null → Expected number).
        assert_eq!(
            err("shell.create", json!({"command": "ls", "timeout": null})),
            "Expected number\n  at [\"timeout\"]"
        );
        // Missing optional fields are fine; the required one is not.
        assert_eq!(err("websearch.query", json!({})), "Missing key\n  at [\"query\"]");
    }

    #[test]
    fn enforced_slice_is_sorted_and_generated_backed() {
        assert!(ENFORCED_OPS.windows(2).all(|w| w[0] < w[1]));
        for op in ENFORCED_OPS {
            assert!(
                validation_gen::body_node(op).is_some(),
                "{op} enforced but has no generated JSON body"
            );
        }
        // LOC-MW (session-resolution-first) routes must NOT be enforced by
        // the middleware — their payload 400 comes after the 404.
        for op in ["session.update", "session.switchAgent", "session.prompt", "session.shell"] {
            assert!(!enforces(op), "{op} must not be middleware-enforced");
        }
    }
}
