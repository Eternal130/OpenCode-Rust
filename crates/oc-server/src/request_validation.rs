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
//! post-session validation runs IN-HANDLER via
//! [`validate_at_handler`] (RA-01 slice 1: 404 first, then Params →
//! Query → Payload). [`ENFORCED_OPS`] lists only validate-first routes
//! (no location middleware in `protocol/src/groups/*`), where v2
//! rejects wrong payloads regardless of resource existence.
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

/// JS `Number(string)` for the inputs that matter: trims whitespace,
/// `""` → 0, decimal/exponent literals, `0x` hex, `Infinity`; anything
/// else is `NaN`. Migrated from the retired Phase-10 `schema_check`
/// module (RA-01) — the seam's single implementation.
fn js_number(raw: &str) -> f64 {
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
/// Migrated from the retired Phase-10 `schema_check` module (RA-01).
fn parse_query_pairs(query: &str) -> Vec<(String, String)> {
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

/// Body buffering limit: axum's default `DefaultBodyLimit` (2 MiB) — the
/// same budget the handlers' `Json` extractors apply after pass-through.
const BODY_LIMIT: usize = 2 * 1024 * 1024;

/// The HTTP-H0 rollout (COMPLETE 2026-10-21): EVERY validate-first JSON
/// body operation, i.e. every op whose v2 endpoint carries NEITHER
/// `sessionLocationMiddleware` NOR `formLocationMiddleware` (see
/// [`LOCATION_FIRST_OPS`]) — 28 of the 46 JSON-body ops. Sorted; mirrors
/// the generated table.
///
/// Batches 1-2 (2026-10-19/20, lane-h0roll + lane-h0b) grew the list to
/// 25 ops one probe-pinned batch at a time. Batch 3 (2026-10-21, lane-h0)
/// added the remaining three validate-first body ops —
/// `experimental.mcp.add` (discriminator-union `config`, probe round 3),
/// `experimental.session.import` (Session.Info tree, probe round 1), and
/// `rpc.call` (annotated `Rpc.Input` root — whole-root mismatch prints
/// the identifier, probe round 1) — completing the set; the completeness
/// is pinned by `enforced_slice_is_complete`.
///
/// Excluded forever (not "deferred"): the 16 LOC-MW body ops and 2
/// FORM-MW body ops whose v2 endpoints resolve the session/form (404)
/// BEFORE payload decode; their post-location seam is in-handler
/// (`validate_at_handler`, RA-01 slice 1). `session.list`'s query stays
/// middleware-enforced (identical bytes to the retired in-handler P10-E2
/// pin) — see [`NUMBER_PARAMS`].
const ENFORCED_OPS: &[&str] = &[
    "credential.create",
    "credential.update",
    "experimental.config.update",
    "experimental.generate.text",
    "experimental.integration.wellknown.add",
    "experimental.mcp.add",
    "experimental.session.import",
    "integration.command.connect",
    "integration.connect.key",
    "integration.oauth.complete",
    "integration.oauth.connect",
    "plugin.check",
    "plugin.update",
    "project.update",
    "pty.create",
    "pty.update",
    "rpc.call",
    "server.experimental.persistentPty.create",
    "server.experimental.persistentPty.update",
    "session.create",
    "session.environment",
    "session.move",
    "session.view",
    "shell.create",
    "websearch.query",
    "worktree.create",
    "worktree.refresh",
    "worktree.remove",
];

/// Every operation whose v2 endpoint carries a location-resolving
/// middleware (`sessionLocationMiddleware` — `protocol/src/groups/
/// session.ts` (24: 21 session + 3 instructions) and `groups/permission.ts`
/// (3) — or `formLocationMiddleware` — `groups/session.ts` (5)): the
/// session/form resolves (404) BEFORE query/payload decode, so the
/// validate-first middleware must NOT fire for them. Probe-pinned
/// (probe: bad query/body on these routes answers the location 404,
/// never a schema 400). Their post-location seam is the in-handler
/// [`validate_at_handler`] (RA-01 slice 1). Operation ids are the
/// `OpenAPI` identifiers (note the
/// `experimental.` prefixes the endpoint names lack).
pub const LOCATION_FIRST_OPS: &[&str] = &[
    "experimental.session.instructions.entry.list",
    "experimental.session.instructions.entry.put",
    "experimental.session.instructions.entry.remove",
    "experimental.session.skill",
    "experimental.session.wait",
    "session.background",
    "session.command",
    "session.compact",
    "session.fork",
    "session.form.cancel",
    "session.form.create",
    "session.form.get",
    "session.form.list",
    "session.form.reply",
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
    "session.switchAgent",
    "session.switchModel",
    "session.synthetic",
    "session.update",
];

/// Probe-pinned `NumberFromString` integer query params. v2 declares these
/// as `Schema.NumberFromString.pipe(decodeTo(<int filter>))` — decode-time
/// filters the `OpenAPI` spec CANNOT express (the spec renders them as plain
/// strings), so the constraints live here, byte-pinned against the live
/// v2.0.24 reference (`rollout-probe{,-2,-3}.json`):
///
/// ```text
/// limit=abc     → Expected an integer\n  at ["limit"]
/// limit=0       → Expected a value greater than 0            (PositiveInt)
/// limit=0 (msg) → Expected a value greater than or equal to 1 (isGreaterThanOrEqualTo(1))
/// limit=201     → Expected a value less than or equal to 200  (message.list only)
/// context=-1    → Expected a value greater than or equal to 0 (NonNegativeInt)
/// ```
///
/// `(operation_id, param, minimum inclusive, maximum inclusive, exclusive
/// minimum)`. `session.list`'s row duplicates the in-handler P10-E2 pin
/// (identical bytes; middleware fires first).
pub type NumberParam = (&'static str, &'static str, Option<f64>, Option<f64>, Option<f64>);

pub const NUMBER_PARAMS: &[NumberParam] = &[
    ("fs.find", "limit", None, None, Some(0.0)),
    ("server.experimental.persistentPty.read", "lines", None, None, Some(0.0)),
    ("session.list", "limit", None, None, Some(0.0)),
    ("session.message.list", "limit", Some(1.0), Some(200.0), None),
    ("vcs.branch.list", "limit", None, None, Some(0.0)),
    ("vcs.diff", "context", Some(0.0), None, None),
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
        SchemaNode::Union(members) => check_union(members, value, path),
    }
}

/// `SchemaNode::Union`: discriminator-struct unions (probe round 3,
/// `rollout-probe-3.json` — mcp.add `config`). A member whose `type`
/// singleton literal matches the value's `type` member OWNS the check
/// verbatim (`{"type":"local"}` → `Missing key\n  at ["config"]
/// ["command"]`, `{"type":"remote"}` → `Missing key … ["url"]`); with no
/// matching discriminator every member gets a chance and total failure
/// renders the union label Effect prints
/// (`Expected { readonly "type": "local", ... } | …`).
fn check_union(members: &[u32], value: &Value, path: &mut Vec<Seg>) -> Result<(), Reject> {
    if let Some(discriminator) = value.get("type").and_then(Value::as_str) {
        if let Some(candidate) = discriminator_member(members, discriminator) {
            return check(candidate, value, path);
        }
    }
    if members.iter().any(|member| check(*member, value, path).is_ok()) {
        return Ok(());
    }
    let labels: Vec<String> = members.iter().filter_map(|m| union_member_label(*m)).collect();
    if labels.len() != members.len() {
        // A member without a renderable label cannot happen (the
        // generator only emits discriminator-struct unions) — accept
        // rather than print a wrong message.
        return Ok(());
    }
    Err(Reject::at(format!("Expected {}", labels.join(" | ")), path))
}

/// The member whose `type` field is a singleton literal equal to
/// `discriminator` (returns the member's OBJECT node — the `$ref` label
/// must not replace member-internal messages).
fn discriminator_member(members: &[u32], discriminator: &str) -> Option<u32> {
    members.iter().find_map(|member| {
        let node = match &validation_gen::NODES[*member as usize] {
            SchemaNode::Ref { node, .. } => *node,
            SchemaNode::Obj { .. } => *member,
            _ => return None,
        };
        let SchemaNode::Obj { fields } = &validation_gen::NODES[node as usize] else {
            return None;
        };
        let type_field = fields.iter().find(|field| field.name == "type")?;
        match &validation_gen::NODES[type_field.ty as usize] {
            SchemaNode::Literal(values) if values.len() == 1 && values[0] == discriminator => {
                Some(node)
            }
            _ => None,
        }
    })
}

/// Effect's rendering of one discriminator-struct union member:
/// `{ readonly "type": "local", ... }` (probe-pinned byte-for-byte).
fn union_member_label(member: u32) -> Option<String> {
    let node = match &validation_gen::NODES[member as usize] {
        SchemaNode::Ref { node, .. } => *node,
        SchemaNode::Obj { .. } => member,
        _ => return None,
    };
    let SchemaNode::Obj { fields } = &validation_gen::NODES[node as usize] else {
        return None;
    };
    let type_field = fields.iter().find(|field| field.name == "type")?;
    match &validation_gen::NODES[type_field.ty as usize] {
        SchemaNode::Literal(values) if values.len() == 1 => {
            Some(format!("{{ readonly \"type\": {:?}, ... }}", values[0]))
        }
        _ => None,
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
// Query interpreter (URL strings — v2 decodes urlParams BEFORE payloads)
// ---------------------------------------------------------------------------

/// The Effect kind label a query param's DECLARED node prints for a
/// kind mismatch (`?limit=5&limit=abc` yields an array; probe round 3:
/// `Expected string | undefined` for limit/search, `Expected "asc" |
/// "desc" | undefined` for order).
fn query_kind_label(node: u32) -> String {
    match &validation_gen::NODES[node as usize] {
        SchemaNode::Literal(values) => render_literals(values),
        SchemaNode::Ref { label, .. } => (*label).to_owned(),
        SchemaNode::Nullable(inner) => query_kind_label(*inner),
        SchemaNode::Union(members) => {
            members.first().map_or_else(|| "string".to_owned(), |m| query_kind_label(*m))
        }
        // Str, Pattern (raw kind label `string`) and everything else.
        _ => "string".to_owned(),
    }
}

/// `Expected <label>[ | undefined]` — optional params (every query param
/// except vcs.diff `mode`, fs.find `query`, fs.write `path`, worktree.list
/// `projectID`) append Effect's optional-union suffix on KIND mismatches;
/// predicate failures never do (probe: `order=bogus` has no suffix while
/// `location=scalar` → `Expected object | undefined`).
fn query_kind_mismatch(label: &str, name: &str, required: bool) -> String {
    let suffix = if required { "" } else { " | undefined" };
    format!("Expected {label}{suffix}\n  at [{name:?}]")
}

fn render_literals(values: &[&str]) -> String {
    values.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(" | ")
}

/// Whether a query param is a struct param (the deepObject `location`):
/// declared as an object; URL spelling is `name[member]=value`.
fn is_struct_param(node: u32) -> bool {
    let node = match &validation_gen::NODES[node as usize] {
        SchemaNode::Ref { node, .. } => *node,
        other => return matches!(other, SchemaNode::Obj { .. } | SchemaNode::Record(_)),
    };
    matches!(&validation_gen::NODES[node as usize], SchemaNode::Obj { .. } | SchemaNode::Record(_))
}

/// Validates one operation's query string against the generated query
/// row. String semantics (URL values are always strings; probes
/// `rollout-probe{,-2,-3}.json`):
///
/// * required param absent → `Missing key\n  at ["mode"]` (kind Query);
/// * struct param present ONLY as scalars (`location=x`) →
///   `Expected object | undefined\n  at ["location"]`; deepObject members
///   are strings (nothing fails) and duplicates last-win (probe r3);
/// * duplicated scalar params (array value) → kind label + `| undefined`;
/// * `Literal` miss → `Expected "asc" | "desc"`; `$ref`'d literal → the
///   identifier (`Expected Vcs.Mode`); `Pattern` miss →
///   `Expected a string starting with "msg_"`;
/// * string-unions (session.list `parentID`) accept any member and report
///   the FIRST member's message (`^ses`, probe r1);
/// * [`NUMBER_PARAMS`] carry the `NumberFromString` integer filters
///   (`Expected an integer` / bound messages).
///
/// Declaration order wins (probe r2: `?limit=abc&order=bogus` reports
/// limit). Unknown keys are stripped (Effect behavior; probe r2).
pub fn validate_query(operation_id: &str, query: &str) -> Result<(), String> {
    let Some(node) = validation_gen::query_node(operation_id) else {
        return Ok(());
    };
    let fields = match &validation_gen::NODES[node as usize] {
        SchemaNode::Obj { fields } => *fields,
        _ => return Ok(()),
    };
    let pairs = parse_query_pairs(query);
    for field in fields {
        let name = field.name;
        let deep_prefix = format!("{name}[");
        let has_deep = pairs.iter().any(|(key, _)| key.starts_with(&deep_prefix));
        let plain: Vec<&str> =
            pairs.iter().filter(|(key, _)| key == name).map(|(_, value)| value.as_str()).collect();

        if is_struct_param(field.ty) {
            if has_deep {
                // Assembled object: members are strings (today only
                // `directory`), unknown members stripped, duplicate
                // members last-win (probe r3) — nothing can fail.
                continue;
            }
            if plain.is_empty() {
                if field.required {
                    return Err(format!("Missing key\n  at [{name:?}]"));
                }
                continue;
            }
            // Scalar (or repeated scalar → array) where an object is
            // declared (probe r1/r2: `location=scalar`).
            return Err(query_kind_mismatch("object", name, field.required));
        }

        if plain.is_empty() {
            if field.required {
                return Err(format!("Missing key\n  at [{name:?}]"));
            }
            continue;
        }
        if plain.len() > 1 {
            // Repeated scalar → Effect sees an array → kind mismatch.
            return Err(query_kind_mismatch(&query_kind_label(field.ty), name, field.required));
        }
        let value = plain[0];

        // NumberFromString filters first (the table row is a plain Str —
        // the integer bounds are decode-time in v2 and spec-invisible).
        if let Some((minimum, maximum, exclusive_minimum)) =
            NUMBER_PARAMS.iter().find_map(|(op, param, min, max, excl)| {
                (*op == operation_id && *param == name).then_some((*min, *max, *excl))
            })
        {
            check_number_param(value, name, minimum, maximum, exclusive_minimum)?;
        }

        match &validation_gen::NODES[field.ty as usize] {
            SchemaNode::Pattern { prefix } => {
                if !value.starts_with(prefix) {
                    return Err(format!(
                        "Expected a string starting with {prefix:?}\n  at [{name:?}]"
                    ));
                }
            }
            SchemaNode::Literal(values) => {
                if !values.contains(&value) {
                    return Err(format!("Expected {}\n  at [{name:?}]", render_literals(values)));
                }
            }
            SchemaNode::Ref { label, node } => {
                // Probe: `mode=bogus` prints the $ref identifier
                // (`Expected Vcs.Mode`), not the literal union.
                match &validation_gen::NODES[*node as usize] {
                    SchemaNode::Literal(values) if !values.contains(&value) => {
                        return Err(format!("Expected {label}\n  at [{name:?}]"));
                    }
                    SchemaNode::Pattern { prefix } if !value.starts_with(prefix) => {
                        return Err(format!(
                            "Expected a string starting with {prefix:?}\n  at [{name:?}]"
                        ));
                    }
                    _ => {}
                }
            }
            SchemaNode::Union(members) => {
                // String-union (session.list parentID): accept on any
                // member; report the FIRST member's message otherwise
                // (probe r1: `Expected a string starting with "ses"`).
                let accepted =
                    members.iter().any(|member| query_union_member_accepts(*member, value));
                let rejection = (!accepted)
                    .then(|| {
                        members
                            .first()
                            .map(|member| query_union_member_message(*member, value, name))
                    })
                    .flatten();
                if let Some(message) = rejection {
                    return Err(message);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Whether one string-union member accepts `value` verbatim.
fn query_union_member_accepts(member: u32, value: &str) -> bool {
    match &validation_gen::NODES[member as usize] {
        SchemaNode::Str => true,
        SchemaNode::Pattern { prefix } => value.starts_with(prefix),
        SchemaNode::Literal(values) => values.contains(&value),
        _ => false,
    }
}

/// The message a string-union member produces for a non-matching value.
fn query_union_member_message(member: u32, value: &str, name: &str) -> String {
    match &validation_gen::NODES[member as usize] {
        SchemaNode::Pattern { prefix } => {
            format!("Expected a string starting with {prefix:?}\n  at [{name:?}]")
        }
        SchemaNode::Literal(values) => {
            format!("Expected {}\n  at [{name:?}]", render_literals(values))
        }
        _ => format!("Expected string\n  at [{name:?}] ({value} rejected)"),
    }
}

/// One probe-pinned `NumberFromString` param (`Expected an integer`,
/// `Expected a value greater than [or equal to] N`,
/// `Expected a value less than or equal to N`).
fn check_number_param(
    value: &str,
    name: &str,
    minimum: Option<f64>,
    maximum: Option<f64>,
    exclusive_minimum: Option<f64>,
) -> Result<(), String> {
    let number = js_number(value);
    if !number.is_finite() || number.fract() != 0.0 {
        return Err(format!("Expected an integer\n  at [{name:?}]"));
    }
    if let Some(bound) = exclusive_minimum {
        if number <= bound {
            return Err(format!(
                "Expected a value greater than {}\n  at [{name:?}]",
                fmt_bound(bound)
            ));
        }
    }
    if let Some(bound) = minimum {
        if number < bound {
            return Err(format!(
                "Expected a value greater than or equal to {}\n  at [{name:?}]",
                fmt_bound(bound)
            ));
        }
    }
    if let Some(bound) = maximum {
        if number > bound {
            return Err(format!(
                "Expected a value less than or equal to {}\n  at [{name:?}]",
                fmt_bound(bound)
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Middleware (the v2 SchemaErrorMiddleware position)
// ---------------------------------------------------------------------------

/// Whether the middleware enforces this operation's JSON payload
/// (validate-first body slice — the complete set; see [`ENFORCED_OPS`]).
#[must_use]
pub fn enforces(operation_id: &str) -> bool {
    ENFORCED_OPS.binary_search(&operation_id).is_ok()
}

/// Whether the operation's v2 endpoint resolves the session/form BEFORE
/// decoding (404 precedes any schema 400 — see [`LOCATION_FIRST_OPS`]).
#[must_use]
pub fn location_first(operation_id: &str) -> bool {
    LOCATION_FIRST_OPS.binary_search(&operation_id).is_ok()
}

/// Whether the middleware enforces this operation's query string: it
/// declares query params (generated `QueryValidation::Params` row) AND is
/// not location-first. Query decode precedes payload decode in v2 (probe:
/// `POST /api/websearch?location=scalar` + bad body → the Query error).
#[must_use]
pub fn enforces_query(operation_id: &str) -> bool {
    validation_gen::query_node(operation_id).is_some() && !location_first(operation_id)
}

/// Whether the middleware enforces this operation's path params: it
/// declares at least one `^literal` pattern param AND is not
/// location-first. v2 decodes path params FIRST (Params → Query →
/// Payload — the location middleware reads the RAW `route.params`, so
/// its 404 precedes any Params 400 on location-first routes).
#[must_use]
pub fn enforces_params(operation_id: &str) -> bool {
    !validation_gen::path_params(operation_id).is_empty() && !location_first(operation_id)
}

/// Validates one operation's path params against the generated pattern
/// rows. A value that misses the `^literal` startsWith filter rejects
/// with the pinned TreeFormatter message (kind `Params`); missing params
/// cannot happen (the router matched the path shape).
pub fn validate_params(operation_id: &str, params: &[(&str, &str)]) -> Result<(), String> {
    for spec in validation_gen::path_params(operation_id) {
        let Some((_, value)) = params.iter().find(|(name, _)| *name == spec.name) else {
            continue;
        };
        if !value.starts_with(spec.prefix) {
            return Err(format!(
                "Expected a string starting with {:?}\n  at [{:?}]",
                spec.prefix, spec.name
            ));
        }
    }
    Ok(())
}

/// The post-location validation seam (RA-01 slice 1) for
/// [`LOCATION_FIRST_OPS`] handlers: v2 resolves the session/form (404)
/// in location middleware BEFORE decoding, so these routes validate
/// Params → Query → Payload only AFTER the in-handler 404 resolution
/// succeeded. Returns the parsed payload (empty body decodes as null —
/// probe r1) so handlers can drop their `Json` extractor (whose axum
/// rejection would otherwise preempt the 404 with a plain-text 400).
pub fn validate_at_handler(
    operation_id: &str,
    method: &axum::http::Method,
    uri: &axum::http::Uri,
    body: &[u8],
) -> Result<Value, crate::errors::ApiError> {
    // 1. path params (v2 decode order: Params → Query → Payload).
    if let Some(table_method) = table_method(method) {
        if let Some((_, raw)) = crate::routes_gen::find(table_method, uri.path()) {
            let params: Vec<(&str, &str)> =
                raw.iter().map(|(name, value)| (*name, value.as_str())).collect();
            if let Err(message) = validate_params(operation_id, &params) {
                return Err(crate::errors::schema_rejection(&message, "Params"));
            }
        }
    }
    // 2. query (urlParams).
    if validation_gen::query_node(operation_id).is_some() {
        if let Err(message) = validate_query(operation_id, uri.query().unwrap_or("")) {
            return Err(crate::errors::schema_rejection(&message, "Query"));
        }
    }
    // 3. payload (only JSON bodies buffer + validate).
    if validation_gen::body_node(operation_id).is_some() {
        let value = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(body).map_err(|_| {
                crate::errors::schema_rejection("Expected a valid JSON body", "Payload")
            })?
        };
        validate_payload(operation_id, &value)
            .map_err(|message| crate::errors::schema_rejection(&message, "Payload"))?;
        return Ok(value);
    }
    Ok(Value::Null)
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

/// v2's plain-text 415 (P10-N1, probe round 1: `Unsupported content-type:
/// text/plain`, response content-type exactly `text/plain`).
fn unsupported_media_type(content_type: &str) -> Response {
    let mut response = (
        axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
        format!("Unsupported content-type: {content_type}"),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, axum::http::HeaderValue::from_static("text/plain"));
    response
}

/// The schema-error middleware — the v2 `SchemaErrorMiddleware` position:
///
/// 1. **Query first** (v2 decodes urlParams before payloads): enforced
///    operations with a generated query row reject bad values as
///    `400 InvalidRequestError {message, kind: "Query"}`.
/// 2. **Content-type gate** (P10-N1, probe round 1/2): JSON-body routes
///    answer `415 Unsupported content-type: <ct>` for any non-JSON
///    content-type (absent is treated as JSON and validated); the binary
///    route (experimental.fs.write) requires `application/octet-stream`
///    and names `application/json` in the 415 when the header is absent.
///    Location-first routes skip the gate — their 404 precedes it.
/// 3. **Payload**: empty bodies decode as null (`Expected object`),
///    unparseable JSON answers `Expected a valid JSON body`, schema
///    violations the byte-pinned Effect message — all
///    `400 InvalidRequestError {…, kind: "Payload"}`.
///
/// Everything else passes through byte-identically — including
/// location-first routes (their 404 must precede any 400/415).
pub async fn schema_error_middleware(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let query = request.uri().query().map(str::to_owned);
    let operation_id = table_method(&method)
        .and_then(|table_method| {
            crate::routes_gen::find(table_method, &path).map(|(route, _)| route.operation_id)
        })
        .filter(|operation_id| {
            enforces(operation_id) || enforces_query(operation_id) || enforces_params(operation_id)
                || // Binary-body routes still take the content-type gate.
                (validation_gen::find(operation_id)
                    .is_some_and(|row| row.body == validation_gen::BodyValidation::Binary)
                    && !location_first(operation_id))
        });
    let Some(operation_id) = operation_id else {
        return next.run(request).await;
    };

    // 0. path params decode FIRST in v2 (Params → Query → Payload; the
    //    generated `^literal` id filters, kind `Params`).
    if enforces_params(operation_id) {
        if let Some(table_method) = table_method(&method) {
            if let Some((_, raw)) = crate::routes_gen::find(table_method, &path) {
                let params: Vec<(&str, &str)> =
                    raw.iter().map(|(name, value)| (*name, value.as_str())).collect();
                if let Err(message) = validate_params(operation_id, &params) {
                    return crate::errors::schema_rejection(&message, "Params").into_response();
                }
            }
        }
    }

    // 1. query (urlParams) before payload — probe: websearch?location=x.
    //    An absent query string is the EMPTY param set: required params
    //    still 400 (`Missing key`, probe r2: bare /api/vcs/diff).
    if enforces_query(operation_id) {
        if let Err(message) = validate_query(operation_id, query.as_deref().unwrap_or("")) {
            return crate::errors::schema_rejection(&message, "Query").into_response();
        }
    }

    let body_kind = validation_gen::find(operation_id).map(|row| row.body);
    if body_kind.is_none_or(|kind| kind == validation_gen::BodyValidation::None) {
        return next.run(request).await;
    }
    // Location-first routes resolve the session (404) before any payload
    // decode — no gate, no buffering.
    if location_first(operation_id) {
        return next.run(request).await;
    }

    // 2. content-type gate (P10-N1).
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    match body_kind {
        Some(validation_gen::BodyValidation::Binary) => match content_type.as_deref() {
            Some(ct) if ct.starts_with("application/octet-stream") => {
                return next.run(request).await;
            }
            // Probe r2: absent content-type names the JSON default.
            None => return unsupported_media_type("application/json"),
            Some(ct) => return unsupported_media_type(ct),
        },
        Some(validation_gen::BodyValidation::Json(_)) => match content_type.as_deref() {
            None => {}
            Some(ct) if ct.starts_with("application/json") => {}
            Some(ct) => return unsupported_media_type(ct),
        },
        _ => {}
    }

    // 3. payload (only JSON bodies buffer + validate).
    let Some(validation_gen::BodyValidation::Json(_)) = body_kind else {
        return next.run(request).await;
    };
    if !enforces(operation_id) {
        return next.run(request).await;
    }
    let (mut parts, body) = request.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, BODY_LIMIT).await else {
        // Over the same 2 MiB budget the `Json` extractor would have
        // applied; mirror its plain-text 413.
        return (axum::http::StatusCode::PAYLOAD_TOO_LARGE, "length limit exceeded")
            .into_response();
    };
    let value = if bytes.is_empty() {
        // Probe r1: empty body decodes as undefined/null → the root
        // struct's own message (`Expected object`).
        Value::Null
    } else {
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => value,
            // Probe r1: `Expected a valid JSON body` (kind Payload).
            Err(_) => {
                return crate::errors::schema_rejection("Expected a valid JSON body", "Payload")
                    .into_response();
            }
        }
    };
    match validate_payload(operation_id, &value) {
        Ok(()) => {
            // Absent content-type defaults to JSON (probe r1) — insert it
            // so the handler's `Json` extractor accepts the re-inserted
            // body the way v2 decoded it.
            if content_type.is_none() {
                parts.headers.insert(
                    header::CONTENT_TYPE,
                    axum::http::HeaderValue::from_static("application/json"),
                );
            }
            let request = Request::from_parts(parts, Body::from(bytes));
            next.run(request).await
        }
        Err(message) => crate::errors::schema_rejection(&message, "Payload").into_response(),
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
    /// condition 2): enforcement for these routes landed with batch 2
    /// (2026-10-20) — except `session.form.reply`, which carries v2
    /// `formLocationMiddleware` (form resolution precedes payload decode)
    /// and stays out until the post-location seam.
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

    /// Batch-2 ops (2026-10-20 probe, `env-requiredness-probe.json`):
    /// wrong-type + missing-key per route, byte-pinned against a live
    /// v2.0.24 boot BEFORE the `ENFORCED_OPS` edit landed. Includes the
    /// env-requiredness adjudication rows for the re-enabled rollback
    /// pair (v2: persistent variant SPEC-STRICT — env-less create 400s;
    /// ephemeral variant lenient — env-less create 200s).
    #[test]
    fn pinned_batch2_messages() {
        // pty.create (no required fields — wrong-type probes only)
        assert_eq!(err("pty.create", json!({"command": 5})), "Expected string\n  at [\"command\"]");
        assert_eq!(err("pty.create", json!({"args": [5]})), "Expected string\n  at [\"args\"][0]");
        // server.experimental.persistentPty.create: the adjudication
        // rows (declaration order args → title → env; env-less create
        // is the CI-rollback case).
        assert_eq!(
            err(
                "server.experimental.persistentPty.create",
                json!({"args": ["-c", "exit 0"], "title": "t"})
            ),
            "Missing key\n  at [\"env\"]"
        );
        assert_eq!(
            err(
                "server.experimental.persistentPty.create",
                json!({"args": ["-c", "exit 0"], "title": "t", "env": 5})
            ),
            "Expected object\n  at [\"env\"]"
        );
        // credential.create (declaration order id? → integrationID! →
        // label? → value! → activate?)
        assert_eq!(
            err("credential.create", json!({"integrationID": 5})),
            "Expected string\n  at [\"integrationID\"]"
        );
        assert_eq!(err("credential.create", json!({})), "Missing key\n  at [\"integrationID\"]");
        assert_eq!(
            err("credential.create", json!({"integrationID": "int_h0b"})),
            "Missing key\n  at [\"value\"]"
        );
        // integration.connect.key / integration.oauth.connect
        assert_eq!(
            err("integration.connect.key", json!({"key": 5})),
            "Expected string\n  at [\"key\"]"
        );
        assert_eq!(err("integration.connect.key", json!({})), "Missing key\n  at [\"key\"]");
        assert_eq!(
            err("integration.oauth.connect", json!({"methodID": 5})),
            "Expected string\n  at [\"methodID\"]"
        );
        assert_eq!(err("integration.oauth.connect", json!({})), "Missing key\n  at [\"methodID\"]");
        // integration.oauth.complete (nullable optional `code`)
        assert_eq!(
            err("integration.oauth.complete", json!({"code": 5})),
            "Expected string | null\n  at [\"code\"]"
        );
        // plugin.check (nullable optional `target`)
        assert_eq!(
            err("plugin.check", json!({"target": 5})),
            "Expected string | null\n  at [\"target\"]"
        );
        // worktree.refresh / worktree.remove
        assert_eq!(
            err("worktree.refresh", json!({"projectID": 5})),
            "Expected string\n  at [\"projectID\"]"
        );
        assert_eq!(err("worktree.refresh", json!({})), "Missing key\n  at [\"projectID\"]");
        assert_eq!(
            err("worktree.remove", json!({"projectID": 5})),
            "Expected string\n  at [\"projectID\"]"
        );
        assert_eq!(
            err("worktree.remove", json!({"projectID": "prj_h0b"})),
            "Missing key\n  at [\"directory\"]"
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
            // Batch-2 pass-through pins (probe: v2 accepts all of these
            // — including the env-less pty.create, the adjudication's
            // lenient half).
            ("pty.create", json!({"command": "sh", "args": ["-c", "exit 0"]})),
            ("pty.create", json!({})),
            ("pty.create", json!({"env": {"K": "v"}})),
            (
                "server.experimental.persistentPty.create",
                json!({"args": ["-c", "exit 0"], "title": "t", "env": {}}),
            ),
            ("credential.create", json!({"integrationID": "int_h0b", "value": {"k": 1}})),
            ("integration.connect.key", json!({"key": "k"})),
            ("integration.oauth.connect", json!({"methodID": "m"})),
            ("integration.oauth.complete", json!({})),
            ("integration.oauth.complete", json!({"code": null})),
            ("plugin.check", json!({})),
            ("worktree.refresh", json!({"projectID": "prj_h0b"})),
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
    }

    /// The rollout is COMPLETE (2026-10-21): [`ENFORCED_OPS`] is exactly the
    /// set of JSON-body operations minus the location-first routes
    /// (24 sessionLocationMiddleware + 5 formLocationMiddleware ops, ids
    /// from the `OpenAPI` identifiers). 28 of the 46 JSON-body ops; the
    /// other 18 stay out forever (not deferred — their v2 endpoints
    /// resolve the session/form first, so a validate-first 400 would
    /// DIVERGE from v2's 404).
    #[test]
    fn enforced_slice_is_complete() {
        assert!(LOCATION_FIRST_OPS.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(LOCATION_FIRST_OPS.len(), 29, "24 LOC-MW + 5 FORM-MW");
        let json_body_ops: Vec<&str> = validation_gen::ROUTES
            .iter()
            .filter(|row| matches!(row.body, validation_gen::BodyValidation::Json(_)))
            .map(|row| row.operation_id)
            .collect();
        assert_eq!(json_body_ops.len(), 46, "46 JSON-body operations");
        let validate_first: std::collections::BTreeSet<&str> =
            json_body_ops.into_iter().filter(|op| !location_first(op)).collect();
        let enforced: std::collections::BTreeSet<&str> = ENFORCED_OPS.iter().copied().collect();
        assert_eq!(
            enforced, validate_first,
            "ENFORCED_OPS must be the complete validate-first body set"
        );
        // The 18 excluded body ops are precisely the location-first ones.
        let excluded: std::collections::BTreeSet<&str> = validation_gen::ROUTES
            .iter()
            .filter(|row| {
                matches!(row.body, validation_gen::BodyValidation::Json(_))
                    && location_first(row.operation_id)
            })
            .map(|row| row.operation_id)
            .collect();
        assert_eq!(excluded.len(), 18, "16 LOC-MW + 2 FORM-MW body ops");
        for op in &excluded {
            assert!(!enforces(op), "{op} is location-first: the 404 must precede its payload 400");
            assert!(!enforces_query(op), "{op} query decode also waits for the 404");
        }
        // Every location-first id exists in the generated table (no vacuous
        // entries — the batch-2 list had `session.skill`/`session.wait`
        // instead of the `experimental.`-prefixed identifiers).
        let known: std::collections::BTreeSet<&str> =
            validation_gen::ROUTES.iter().map(|row| row.operation_id).collect();
        for op in LOCATION_FIRST_OPS {
            assert!(known.contains(op), "{op} is not a generated operation id");
        }
        // The raw-handshake routes (pty.connect / persistentPty.connect)
        // declare query rows but nothing can fail (plain strings).
        assert!(enforces_query("pty.connect"));
        assert!(enforces_query("persistentPty.connect"));
    }

    /// Query-enforcement coverage: every generated query row is enforced
    /// except the two location-first query ops (probe: their 404 wins).
    #[test]
    fn query_enforcement_coverage() {
        let query_ops: Vec<&str> = validation_gen::ROUTES
            .iter()
            .filter(|row| matches!(row.query, validation_gen::QueryValidation::Params(_)))
            .map(|row| row.operation_id)
            .collect();
        assert_eq!(query_ops.len(), validation_gen::QUERY_PARAM_ROUTES);
        assert_eq!(validation_gen::QUERY_PARAM_ROUTES, 71);
        let enforced: usize = query_ops.iter().filter(|op| enforces_query(op)).copied().count();
        assert_eq!(enforced, 69, "71 query rows - session.interrupt - session.form.cancel");
        for op in ["session.interrupt", "session.form.cancel"] {
            assert!(!enforces_query(op), "{op} is location-first");
        }
    }

    /// Batch-3 body ops (probe round 1/3, `rollout-probe{,-3}.json`):
    /// the discriminator-union `config`, the Session.Info import tree,
    /// and the annotated Rpc.Input root.
    #[test]
    fn pinned_batch3_messages() {
        // experimental.mcp.add — union label, discriminator delegation.
        assert_eq!(err("experimental.mcp.add", json!({})), "Missing key\n  at [\"config\"]");
        assert_eq!(err("experimental.mcp.add", json!([1])), "Expected object");
        assert_eq!(
            err("experimental.mcp.add", json!({"config": 5})),
            "Expected { readonly \"type\": \"local\", ... } | { readonly \"type\": \"remote\", ... }\n  \
               at [\"config\"]"
        );
        assert_eq!(
            err("experimental.mcp.add", json!({"config": {"type": "bogus"}})),
            "Expected { readonly \"type\": \"local\", ... } | { readonly \"type\": \"remote\", ... }\n  \
               at [\"config\"]"
        );
        assert_eq!(
            err("experimental.mcp.add", json!({"config": {"type": "local"}})),
            "Missing key\n  at [\"config\"][\"command\"]"
        );
        assert_eq!(
            err("experimental.mcp.add", json!({"config": {"type": "local", "command": "x"}})),
            "Expected array\n  at [\"config\"][\"command\"]"
        );
        assert_eq!(
            err("experimental.mcp.add", json!({"config": {"type": "remote"}})),
            "Missing key\n  at [\"config\"][\"url\"]"
        );
        assert!(validate_payload(
            "experimental.mcp.add",
            &json!({"config": {"type": "local", "command": ["true"]}})
        )
        .is_ok());

        // experimental.session.import — Session.Info tree.
        assert_eq!(err("experimental.session.import", json!({})), "Missing key\n  at [\"info\"]");
        assert_eq!(err("experimental.session.import", json!([1])), "Expected object");
        assert_eq!(
            err("experimental.session.import", json!({"info": 5})),
            "Expected Session.Info\n  at [\"info\"]"
        );
        assert_eq!(
            err("experimental.session.import", json!({"info": {}, "messages": []})),
            "Missing key\n  at [\"info\"][\"id\"]"
        );

        // rpc.call — annotated root (probe: the identifier, not `object`).
        assert_eq!(err("rpc.call", json!([1])), "Expected Rpc.Input");
        assert!(validate_payload("rpc.call", &json!({})).is_ok());
        assert!(validate_payload("rpc.call", &json!({"input": 5})).is_ok());
    }

    /// Query interpreter pins (probe rounds 1-3, byte-level).
    #[allow(clippy::too_many_lines)] // one pinned table, one test
    #[test]
    fn pinned_query_messages() {
        let q = |op: &str, query: &str| validate_query(op, query).unwrap_err();
        // deepObject location: scalar → `Expected object | undefined`;
        // deep members + unknown members + duplicates never fail.
        assert_eq!(
            q("config.get", "location=scalar"),
            "Expected object | undefined\n  at [\"location\"]"
        );
        assert!(validate_query("config.get", "location%5Bdirectory%5D=%2Ftmp").is_ok());
        assert!(validate_query("config.get", "location%5Bbogus%5D=x").is_ok());
        assert!(validate_query("config.get", "location=scalar&location%5Bdirectory%5D=x").is_ok());
        assert!(validate_query("config.get", "").is_ok());

        // session.list: NumberFromString, literal union, string-union.
        assert_eq!(q("session.list", "limit=abc"), "Expected an integer\n  at [\"limit\"]");
        assert_eq!(
            q("session.list", "limit=0"),
            "Expected a value greater than 0\n  at [\"limit\"]"
        );
        assert_eq!(
            q("session.list", "limit="),
            "Expected a value greater than 0\n  at [\"limit\"]"
        );
        assert_eq!(
            q("session.list", "order=bogus"),
            "Expected \"asc\" | \"desc\"\n  at [\"order\"]"
        );
        assert_eq!(
            q("session.list", "parentID=bogus"),
            "Expected a string starting with \"ses\"\n  at [\"parentID\"]"
        );
        assert!(validate_query("session.list", "parentID=null").is_ok());
        assert!(validate_query("session.list", "parentID=ses_x").is_ok());
        assert!(validate_query("session.list", "limit=%2B5").is_ok());
        assert!(validate_query("session.list", "limit=0x10").is_ok());
        assert!(validate_query("session.list", "bogus=1").is_ok());
        // Declaration order: limit before order regardless of URL order.
        assert_eq!(
            q("session.list", "order=bogus&limit=abc"),
            "Expected an integer\n  at [\"limit\"]"
        );
        // Duplicates → array → kind mismatch with the optional suffix.
        assert_eq!(
            q("session.list", "limit=5&limit=abc"),
            "Expected string | undefined\n  at [\"limit\"]"
        );
        assert_eq!(
            q("session.list", "order=asc&order=desc"),
            "Expected \"asc\" | \"desc\" | undefined\n  at [\"order\"]"
        );

        // session.message.list: bounded limit + 10-literal type union.
        assert_eq!(
            q("session.message.list", "limit=0"),
            "Expected a value greater than or equal to 1\n  at [\"limit\"]"
        );
        assert_eq!(
            q("session.message.list", "limit=201"),
            "Expected a value less than or equal to 200\n  at [\"limit\"]"
        );
        assert!(validate_query("session.message.list", "limit=200").is_ok());
        assert_eq!(
            q("session.message.list", "type=bogus"),
            "Expected \"agent-switched\" | \"model-switched\" | \"location-switched\" | \"user\" | \
             \"synthetic\" | \"system\" | \"skill\" | \"shell\" | \"assistant\" | \"compaction\"\n  \
             at [\"type\"]"
        );
        assert!(validate_query("session.message.list", "type=user").is_ok());

        // patterns (^msg_) and enums elsewhere.
        assert_eq!(
            q("session.diff", "from=notmsg"),
            "Expected a string starting with \"msg_\"\n  at [\"from\"]"
        );
        assert_eq!(
            q("session.diff", "from=msg_1&to=notmsg"),
            "Expected a string starting with \"msg_\"\n  at [\"to\"]"
        );
        assert!(validate_query("session.diff", "from=msg_1").is_ok());
        assert_eq!(
            q("experimental.session.stats", "tools=bogus"),
            "Expected \"none\" | \"summary\" | \"detail\"\n  at [\"tools\"]"
        );
        assert_eq!(
            q("experimental.session.export", "sanitize=bogus"),
            "Expected \"true\" | \"false\"\n  at [\"sanitize\"]"
        );
        assert_eq!(
            q("session.log", "follow=bogus"),
            "Expected \"true\" | \"false\"\n  at [\"follow\"]"
        );

        // required keys (kind Query — probe r2) and the $ref'd literal.
        assert_eq!(q("vcs.diff", ""), "Missing key\n  at [\"mode\"]");
        assert_eq!(q("vcs.diff", "mode=bogus"), "Expected Vcs.Mode\n  at [\"mode\"]");
        assert!(validate_query("vcs.diff", "mode=working").is_ok());
        assert_eq!(
            q("vcs.diff", "mode=working&context=abc"),
            "Expected an integer\n  at [\"context\"]"
        );
        assert_eq!(
            q("vcs.diff", "mode=working&context=-1"),
            "Expected a value greater than or equal to 0\n  at [\"context\"]"
        );
        assert_eq!(q("fs.find", ""), "Missing key\n  at [\"query\"]");
        assert_eq!(
            q("fs.find", "query=x&type=bogus"),
            "Expected \"file\" | \"directory\"\n  at [\"type\"]"
        );
        assert_eq!(q("fs.find", "query=x&limit=abc"), "Expected an integer\n  at [\"limit\"]");
        assert_eq!(q("worktree.list", ""), "Missing key\n  at [\"projectID\"]");
        assert!(validate_query("worktree.list", "projectID=prj_x").is_ok());
        assert_eq!(
            q("fs.find", "query=x&limit=0"),
            "Expected a value greater than 0\n  at [\"limit\"]"
        );
        assert_eq!(
            q("server.experimental.persistentPty.read", "lines=abc"),
            "Expected an integer\n  at [\"lines\"]"
        );
        assert_eq!(
            q("server.experimental.persistentPty.read", "lines=0"),
            "Expected a value greater than 0\n  at [\"lines\"]"
        );
        assert_eq!(q("vcs.branch.list", "limit=abc"), "Expected an integer\n  at [\"limit\"]");
        // Raw flat-string routes: nothing can fail.
        assert!(
            validate_query("pty.connect", "location%5Bdirectory%5D=x&cursor=1&ticket=t").is_ok()
        );
        assert!(validate_query("persistentPty.connect", "cursor=1&role=r").is_ok());
        // Location-first ops are never query-enforced by the middleware
        // (probe: their 404 wins); the generated row exists but is not
        // consulted — `enforces_query` is the gate.
        assert!(!enforces_query("session.interrupt"));
        assert!(!enforces_query("session.form.cancel"));
    }

    /// Migrated from the retired `handlers/schema_check` module (RA-01):
    /// JS `Number()` semantics the `NumberFromString` filters rely on.
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

    /// Path-param (kind Params) pins: the generated `^literal` id
    /// filters, TreeFormatter message shape, and the location-first
    /// carve-out (their 404 precedes any Params 400).
    #[test]
    fn pinned_params_messages() {
        assert_eq!(
            validate_params("session.get", &[("sessionID", "not_ses")]).unwrap_err(),
            "Expected a string starting with \"ses\"\n  at [\"sessionID\"]"
        );
        assert!(validate_params("session.get", &[("sessionID", "ses_1")]).is_ok());
        assert_eq!(
            validate_params("session.message.get", &[("sessionID", "ses_1"), ("messageID", "x")])
                .unwrap_err(),
            "Expected a string starting with \"msg_\"\n  at [\"messageID\"]"
        );
        assert_eq!(
            validate_params("shell.get", &[("id", "cmd_1")]).unwrap_err(),
            "Expected a string starting with \"sh_\"\n  at [\"id\"]"
        );
        // Middleware-side gate: validate-first ops enforce, location-first
        // ops never do here (the 404 wins; their handlers validate
        // post-location instead).
        assert!(enforces_params("session.get"));
        assert!(enforces_params("shell.get"));
        assert!(enforces_params("pty.get"));
        assert!(enforces_params("session.environment"));
        assert!(!enforces_params("session.prompt"));
        assert!(!enforces_params("session.form.get"));
        assert!(!enforces_params("session.list"));
    }

    /// The post-location seam (RA-01 slice 1): Params → Query → Payload
    /// AFTER the handler resolved the session/form (the caller's 404).
    #[test]
    fn validate_at_handler_ordering_and_messages() {
        let uri: axum::http::Uri = "/api/session/ses_1/prompt".parse().unwrap();
        // Malformed JSON → the pinned plain message, kind Payload.
        let error = validate_at_handler("session.prompt", &axum::http::Method::POST, &uri, b"{")
            .unwrap_err();
        assert_eq!(error.status.as_u16(), 400);
        assert_eq!(error.body["message"], "Expected a valid JSON body");
        assert_eq!(error.body["kind"], "Payload");
        // Schema violation → the TreeFormatter message.
        let error = validate_at_handler(
            "session.prompt",
            &axum::http::Method::POST,
            &uri,
            br#"{"text": 5}"#,
        )
        .unwrap_err();
        assert_eq!(error.body["message"], "Expected string\n  at [\"text\"]");
        assert_eq!(error.body["kind"], "Payload");
        // Empty body decodes as null → the root message (probe r1).
        let error = validate_at_handler("session.prompt", &axum::http::Method::POST, &uri, b"")
            .unwrap_err();
        assert_eq!(error.body["message"], "Expected object");
        // Path params validate before the payload.
        let bad_id: axum::http::Uri = "/api/session/ses_1/inbox/notmsg".parse().unwrap();
        let error =
            validate_at_handler("session.inbox.update", &axum::http::Method::PATCH, &bad_id, b"{")
                .unwrap_err();
        assert_eq!(
            error.body["message"],
            "Expected a string starting with \"msg_\"\n  at [\"inboxID\"]"
        );
        assert_eq!(error.body["kind"], "Params");
        // Query ops validate their query post-location (interrupt).
        let query: axum::http::Uri = "/api/session/ses_1/interrupt?resume=bogus".parse().unwrap();
        let error =
            validate_at_handler("session.interrupt", &axum::http::Method::POST, &query, &[])
                .unwrap_err();
        assert_eq!(error.body["message"], "Expected \"true\" | \"false\"\n  at [\"resume\"]");
        assert_eq!(error.body["kind"], "Query");
        // Valid bodies parse back to the payload Value.
        let body = validate_at_handler(
            "session.prompt",
            &axum::http::Method::POST,
            &uri,
            br#"{"text": "hi"}"#,
        )
        .unwrap();
        assert_eq!(body["text"], "hi");
    }
}
