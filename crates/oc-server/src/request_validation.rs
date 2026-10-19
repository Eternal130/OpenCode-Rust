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
//! Expected boolean\n  at ["force"]               (boolean field, wrong kind)
//! Expected a string starting with "ses"\n  at ["parentID"]  (^ses pattern)
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
//! Label provenance (round-4 probes,
//! `ref-validation-messages-4.json`): `boolean` renders `Expected boolean`
//! (worktree.remove `force`, credential.create `activate`); `^ses` pattern
//! fields render `Expected a string starting with "ses"` — and predicate
//! (filter) failures inside `anyOf [X, null]` do NOT gain the `| null`
//! suffix (the value matched the member's kind), while kind mismatches do
//! (`{"parentID": 5}` → `Expected string | null`).

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

/// The HTTP-H0 rollout (batch 1 after the oracle's APPROVE WITH
/// CONDITIONS): validate-first operations whose JSON payloads are enforced
/// in middleware (sorted; mirrors the generated table). This is the
/// incremental rollout valve — it grows toward the full validate-first
/// set; LOC-MW routes get their post-session seam in a follow-up lane.
///
/// Batch 1 (2026-10-19, lane-h0roll) added 8 ops — wrong-type +
/// missing-key probes per route byte-pinned against a live v2.0.24 boot
/// (`ref-validation-messages-4.json`) and dual-end verified via the
/// `OC_P10` gate before this list grew. Still excluded: the oracle's
/// deferred trio (`session.form.create`, `experimental.session.import`,
/// `experimental.mcp.add`) and the Bool/Num routes whose labels are now
/// pinned but whose enforcement lands with a later batch
/// (`credential.create`, `worktree.remove`, `session.view`,
/// `session.form.reply`, `integration.connect.key`,
/// `integration.oauth.connect`).
const ENFORCED_OPS: &[&str] = &[
    "credential.update",
    "experimental.config.update",
    "experimental.generate.text",
    "experimental.integration.wellknown.add",
    "integration.command.connect",
    "plugin.update",
    "project.update",
    "pty.update",
    "server.experimental.persistentPty.create",
    "server.experimental.persistentPty.update",
    "session.create",
    "session.environment",
    "session.move",
    "shell.create",
    "websearch.query",
    "worktree.create",
];

// pty.create is NOT enforced: the generated table marks `env` required
// (per the pinned spec) but the runtime contract is unprobed — the port
// handler (and pre-enforcement behavior) accepts an env-less create, and
// CI caught `parity_pty_frames::resized_checkpoint` 400ing on it
// (2026-10-19). Re-probe v2 with an env-less payload before re-enabling;
// tracked as a gap row in the H0 matrix.

/// Every operation whose v2 endpoint carries `sessionLocationMiddleware`
/// (`protocol/src/groups/session.ts` — 18 — and
/// `protocol/src/groups/permission.ts` — 3): the session resolves (404)
/// BEFORE payload decode, so the validate-first middleware must NOT fire
/// for them; their post-session seam is a follow-up slice. Regression pin
/// for the exclusion (oracle condition 4).
#[cfg(test)]
const LOC_MW_OPS: &[&str] = &[
    "session.background",
    "session.command",
    "session.compact",
    "session.fork",
    "session.generate",
    "session.inbox.update",
    "session.interrupt",
    "session.permission.create",
    "session.permission.get",
    "session.permission.reply",
    "session.prompt",
    "session.revert.clear",
    "session.revert.commit",
    "session.revert.stage",
    "session.shell",
    "session.skill",
    "session.switchAgent",
    "session.switchModel",
    "session.synthetic",
    "session.update",
    "session.wait",
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
/// `predicate` marks a filter failure (the value's JSON kind matched but
/// a constraint rejected it — e.g. the `^ses` startsWith pattern): those
/// do NOT gain the `| null` union suffix inside nullable fields (probe:
/// `{"parentID": "not_ses"}` → `Expected a string starting with "ses"`
/// while `{"parentID": 5}` → `Expected string | null`).
struct Reject {
    message: String,
    path: Vec<Seg>,
    predicate: bool,
}

impl Reject {
    fn expected(label: &str, path: &[Seg]) -> Self {
        Self { message: format!("Expected {label}"), path: path.to_vec(), predicate: false }
    }

    fn at(message: String, path: &[Seg]) -> Self {
        Self { message, path: path.to_vec(), predicate: false }
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
        SchemaNode::Pattern { prefix } => check_pattern(prefix, value, path),
        SchemaNode::Bool => {
            // Label pinned vs live v2.0.24 (worktree.remove `force`,
            // credential.create `activate`): `Expected boolean`.
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
                if reject.path.len() == path.len() && !reject.predicate {
                    // The nullable node itself mismatched by KIND: Effect
                    // appends `| null` to the inner expectation. Filter
                    // failures (predicate) keep their own message.
                    Reject {
                        message: format!("{} | null", reject.message),
                        path: reject.path,
                        predicate: false,
                    }
                } else {
                    reject
                }
            })
        }
        SchemaNode::Ref { label, node } => check(*node, value, path).map_err(|reject| {
            if reject.path.len() == path.len() && !reject.predicate {
                // Whole-field kind mismatch: print the annotated identifier.
                Reject { message: format!("Expected {label}"), path: reject.path, predicate: false }
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
        return Err(Reject::at("Expected an integer".to_owned(), path));
    }
    if let Some(bound) = exclusive_minimum {
        if number <= bound {
            return Err(Reject::at(
                format!("Expected a value greater than {}", fmt_bound(bound)),
                path,
            ));
        }
    }
    if let Some(bound) = minimum {
        if number < bound {
            return Err(Reject::at(
                format!("Expected a value greater than or equal to {}", fmt_bound(bound)),
                path,
            ));
        }
    }
    Ok(())
}

/// `SchemaNode::Pattern`: the spec's `^literal` startsWith filter.
/// Non-strings are KIND mismatches (`Expected string`, which gains
/// `| null` inside nullables); strings with the wrong prefix are
/// predicate failures with the pinned Effect message
/// (`Expected a string starting with "ses"` — no `| null`).
fn check_pattern(prefix: &str, value: &Value, path: &[Seg]) -> Result<(), Reject> {
    let Some(text) = value.as_str() else {
        return Err(Reject::expected("string", path));
    };
    if text.starts_with(prefix) {
        Ok(())
    } else {
        Err(Reject {
            message: format!("Expected a string starting with {prefix:?}"),
            path: path.to_vec(),
            predicate: true,
        })
    }
}

/// `SchemaNode::Literal`: inline string-literal union
/// (`Expected "a" | "b"`).
fn check_literal(values: &[&str], value: &Value, path: &[Seg]) -> Result<(), Reject> {
    match value.as_str() {
        Some(text) if values.contains(&text) => Ok(()),
        _ => Err(Reject::at(
            format!(
                "Expected {}",
                values.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(" | ")
            ),
            path,
        )),
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
                    return Err(Reject::at("Missing key".to_owned(), &reject_path));
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
        // `^ses` startsWith patterns (round-4 probe, ref-validation-messages-4):
        // predicate failures keep their own message (no `| null`);
        // kind mismatches on the same field gain `| null`; declaration
        // order puts id before parentID.
        assert_eq!(
            err("session.create", json!({"parentID": "not_ses"})),
            "Expected a string starting with \"ses\"\n  at [\"parentID\"]"
        );
        assert_eq!(
            err("session.create", json!({"id": "not_ses"})),
            "Expected a string starting with \"ses\"\n  at [\"id\"]"
        );
        assert_eq!(
            err("session.create", json!({"id": "x", "parentID": "y"})),
            "Expected a string starting with \"ses\"\n  at [\"id\"]"
        );
        assert_eq!(
            err("session.create", json!({"parentID": 5})),
            "Expected string | null\n  at [\"parentID\"]"
        );
        assert_eq!(
            err("session.create", json!({"id": "ses_ok", "parentID": "not_ses"})),
            "Expected a string starting with \"ses\"\n  at [\"parentID\"]"
        );
        assert!(validate_payload("session.create", &json!({"id": "ses_ok"})).is_ok());

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

    /// Bool/Num labels pinned vs live v2.0.24 (round-4 probe; oracle
    /// condition 2): these routes stay OUT of `ENFORCED_OPS` until their
    /// batch, but the interpreter's labels are already byte-pinned.
    #[test]
    fn pinned_boolean_and_number_labels() {
        assert_eq!(
            err(
                "worktree.remove",
                json!({"projectID": "prj_P", "directory": "/tmp/x", "force": 5})
            ),
            "Expected boolean\n  at [\"force\"]"
        );
        assert_eq!(
            err(
                "credential.create",
                json!({"integrationID": "int_P", "value": {"type": "key", "key": "v"}, "activate": 5})
            ),
            "Expected boolean\n  at [\"activate\"]"
        );
        assert_eq!(err("session.view", json!({"idle": "x"})), "Expected number\n  at [\"idle\"]");
        assert_eq!(err("session.view", json!({"idle": true})), "Expected number\n  at [\"idle\"]");
        assert_eq!(err("session.view", json!({})), "Missing key\n  at [\"idle\"]");
        assert_eq!(err("worktree.remove", json!({})), "Missing key\n  at [\"projectID\"]");
    }

    /// Batch-1 ops (round-4 probe): wrong-type + missing-key per route,
    /// byte-pinned against live v2.0.24 BEFORE the `ENFORCED_OPS` edit.
    #[test]
    fn pinned_batch1_messages() {
        // experimental.generate.text
        assert_eq!(
            err("experimental.generate.text", json!({"prompt": 5})),
            "Expected string\n  at [\"prompt\"]"
        );
        assert_eq!(err("experimental.generate.text", json!({})), "Missing key\n  at [\"prompt\"]");
        // experimental.integration.wellknown.add
        assert_eq!(
            err("experimental.integration.wellknown.add", json!({"url": 5})),
            "Expected string\n  at [\"url\"]"
        );
        assert_eq!(
            err("experimental.integration.wellknown.add", json!({})),
            "Missing key\n  at [\"url\"]"
        );
        // experimental.config.update (required nullable shell)
        assert_eq!(
            err("experimental.config.update", json!({"shell": 5})),
            "Expected string | null\n  at [\"shell\"]"
        );
        assert_eq!(err("experimental.config.update", json!({})), "Missing key\n  at [\"shell\"]");
        // integration.command.connect
        assert_eq!(
            err("integration.command.connect", json!({"methodID": 5})),
            "Expected string\n  at [\"methodID\"]"
        );
        assert_eq!(
            err("integration.command.connect", json!({})),
            "Missing key\n  at [\"methodID\"]"
        );
        // session.environment (required record)
        assert_eq!(
            err("session.environment", json!({"variables": 5})),
            "Expected object\n  at [\"variables\"]"
        );
        assert_eq!(err("session.environment", json!({})), "Missing key\n  at [\"variables\"]");
        // server.experimental.persistentPty.create (args array first required)
        assert_eq!(
            err("server.experimental.persistentPty.create", json!({"args": 5})),
            "Expected array\n  at [\"args\"]"
        );
        assert_eq!(
            err("server.experimental.persistentPty.create", json!({})),
            "Missing key\n  at [\"args\"]"
        );
        // server.experimental.persistentPty.update (required inline size)
        assert_eq!(
            err("server.experimental.persistentPty.update", json!({"size": 5})),
            "Expected object\n  at [\"size\"]"
        );
        assert_eq!(
            err("server.experimental.persistentPty.update", json!({})),
            "Missing key\n  at [\"size\"]"
        );
        // worktree.create
        assert_eq!(
            err("worktree.create", json!({"projectID": 5})),
            "Expected string\n  at [\"projectID\"]"
        );
        assert_eq!(err("worktree.create", json!({})), "Missing key\n  at [\"projectID\"]");
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
        // the middleware — their payload 400 comes after the 404. The full
        // 21-op set (18 session + 3 permission, per groups/session.ts and
        // groups/permission.ts), oracle condition 4.
        assert_eq!(LOC_MW_OPS.len(), 21);
        assert!(LOC_MW_OPS.windows(2).all(|w| w[0] < w[1]));
        for op in LOC_MW_OPS {
            assert!(!enforces(op), "{op} is LOC-MW: 404 must precede its payload 400");
        }
        // Deferred trio (oracle): stay out until their batch.
        for op in ["session.form.create", "experimental.session.import", "experimental.mcp.add"] {
            assert!(!enforces(op), "{op} is deferred by oracle review");
        }
    }
}
