//! Emits `crates/oc-server/src/validation.gen.rs`: the generated
//! request-validation constraint table (HTTP-H0).
//!
//! Design (PARITY-LOOP.md §4.5 design gate):
//! * **Data only** — the artifact is a flat schema arena (`NODES`) plus one
//!   row per operation (`ROUTES`, sorted by operation id). No logic lives
//!   here; the interpreter is the hand-written seam
//!   (`oc-server/src/request_validation.rs`).
//! * **Diffable** — deterministic traversal (operations in the same
//!   (path, method) order as `emit_routes`; object fields in spec
//!   DECLARATION order, which Effect's error formatter also uses), arena
//!   entries emitted once per line with their index as a comment.
//! * **Testable** — the artifact carries its own `#[cfg(test)]` unit
//!   tests (coverage vs `routes_gen`, index integrity, slice pins).
//! * **Incremental** — one command regenerates (`cargo run -p openapi-gen`);
//!   `--check` guards drift; unknown JSON-Schema constructs degrade to
//!   `SchemaNode::Any` (accept) so regeneration can never introduce a
//!   false 400 — unrepresentable constraints are COUNTED
//!   (`CAPPED_NODES`), never silently dropped.
//!
//! The spec's `serde_json::Value` parsing sorts object keys, but Effect
//! reports the FIRST offending field in schema-declaration order — so this
//! emitter re-parses the raw document through [`Ordered`], a value type
//! that preserves JSON member order. The rest of the generator pipeline is
//! untouched by that second parse.

use std::collections::{HashMap, HashSet};

use crate::model::Operation;

/// Order-preserving JSON value (serde's map visitor feeds keys in document
/// order; only `serde_json::Map` itself sorts them).
#[derive(Debug, Clone)]
pub enum Ordered {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Ordered>),
    Obj(Vec<(String, Ordered)>),
}

impl<'de> serde::Deserialize<'de> for Ordered {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Ordered;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("any JSON value")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(Ordered::Null)
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(Ordered::Null)
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(Ordered::Bool(v))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                #[allow(clippy::cast_precision_loss)] // JSON numbers here are bounds/ids
                Ok(Ordered::Num(v as f64))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                #[allow(clippy::cast_precision_loss)]
                Ok(Ordered::Num(v as f64))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(Ordered::Num(v))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Ordered::Str(v.to_owned()))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some(item) = seq.next_element()? {
                    out.push(item);
                }
                Ok(Ordered::Arr(out))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some((key, value)) = map.next_entry()? {
                    out.push((key, value));
                }
                Ok(Ordered::Obj(out))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

impl Ordered {
    fn get(&self, key: &str) -> Option<&Ordered> {
        match self {
            Ordered::Obj(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn as_str(&self) -> Option<&str> {
        match self {
            Ordered::Str(s) => Some(s),
            _ => None,
        }
    }
    fn as_f64(&self) -> Option<f64> {
        match self {
            Ordered::Num(n) => Some(*n),
            _ => None,
        }
    }
    fn members(&self) -> &[(String, Ordered)] {
        match self {
            Ordered::Obj(members) => members,
            _ => &[],
        }
    }
    fn items(&self) -> &[Ordered] {
        match self {
            Ordered::Arr(items) => items,
            _ => &[],
        }
    }
    /// Non-empty array members of key `key`.
    fn items_of(&self, key: &str) -> Option<&[Ordered]> {
        self.get(key).map(Ordered::items).filter(|v| !v.is_empty())
    }
}

/// Depth cap for `$ref` resolution: recursive schemas (trees) cannot live
/// in a finite static arena — deeper nodes become `Any` (accept).
const REF_DEPTH_CAP: usize = 8;

/// Intermediate schema IR before arena interning.
#[derive(Debug, Clone, PartialEq)]
enum Ir {
    Any,
    Str,
    /// `{"type":"string","pattern":"^literal"}` — an Effect `startsWith`
    /// filter (message-pinned: `Expected a string starting with "ses"`).
    /// Only metachar-free `^literal` patterns are representable; every
    /// other regex degrades to [`Ir::Str`] (accept, no false reject).
    Pattern {
        prefix: String,
    },
    Int {
        minimum: Option<f64>,
        exclusive_minimum: Option<f64>,
    },
    Num,
    Bool,
    Literal(Vec<String>),
    Ref {
        label: String,
        node: Box<Ir>,
    },
    Nullable(Box<Ir>),
    Arr(Box<Ir>),
    Obj {
        fields: Vec<IrField>,
    },
    Record(Box<Ir>),
    /// Multi-member `anyOf` (no `null` member): emitted ONLY for the two
    /// shapes the seam interpreter can render byte-exactly —
    /// string-unions (all members `Str`/`Pattern`/`Literal`; query
    /// filters like session.list's `parentID`, probe-pinned to the FIRST
    /// member's message) and discriminator-struct unions (every member an
    /// object whose `type` field is a singleton literal, e.g. mcp.add's
    /// `config`, probe-pinned to `{ readonly "type": "local", ... } | …`
    /// plus per-member verbatim errors). Every other union degrades to
    /// `Any` (accept — never a false reject).
    Union(Vec<Ir>),
}

#[derive(Debug, Clone, PartialEq)]
struct IrField {
    name: String,
    required: bool,
    ty: Ir,
}

/// Converts one JSON body schema into IR.
struct Walker<'a> {
    schemas: &'a HashMap<String, Ordered>,
    ref_memo: HashMap<String, Ir>,
    in_progress: HashSet<String>,
    capped: usize,
}

/// A union member renderable by the seam as a string filter
/// (`Str`/`Pattern`/`Literal`) — string-unions report the FIRST member's
/// message (probe: session.list `parentID=bogus` → the `^ses` message).
fn ir_is_stringish(ir: &Ir) -> bool {
    matches!(ir, Ir::Str | Ir::Pattern { .. } | Ir::Literal(_))
}

/// A union member that is (a `$ref` to) an object whose `type` field is a
/// singleton literal — Effect's discriminated-union error rendering
/// (probe: mcp.add `config` → per-member verbatim errors when the
/// discriminator matches, else the union label).
fn ir_is_discriminated(ir: &Ir) -> bool {
    let obj = match ir {
        Ir::Ref { node, .. } => node.as_ref(),
        Ir::Obj { .. } => ir,
        _ => return false,
    };
    let Ir::Obj { fields } = obj else { return false };
    fields.iter().any(|f| f.name == "type" && matches!(&f.ty, Ir::Literal(v) if v.len() == 1))
}

/// Probe-pinned discriminated-union MEMBER SETS: multi-member `anyOf`s
/// the live v2.0.24 reference actually REJECTS on (`rollout-probe-3.json`
/// — mcp.add `config`). A structurally identical union exists whose v2
/// runtime is LENIENT (`Credential.Value` renders anyOf
/// [Credential.OAuth, Credential.Key] yet accepts `{"k":1}` — probe
/// `env-requiredness-probe.json`), so structural detection alone would
/// false-reject; only these member sets may render as `Union`.
const PINNED_UNION_MEMBERS: &[&[&str]] = &[&["Mcp.LocalConfigEncoded", "Mcp.RemoteConfigEncoded"]];

/// The `$ref` labels of a union's members, in order (None if any member
/// is unlabeled — inline structs cannot be matched against the pins).
fn union_member_names(members: &[Ir]) -> Option<Vec<&str>> {
    members
        .iter()
        .map(|m| match m {
            Ir::Ref { label, .. } => Some(label.as_str()),
            _ => None,
        })
        .collect()
}

impl Ordered {
    fn is_null_schema(&self) -> bool {
        self.get("type").and_then(Ordered::as_str) == Some("null")
    }
}

impl Walker<'_> {
    fn walk(&mut self, value: &Ordered, depth: usize) -> Ir {
        // anyOf: the spec uses exactly `[X, {"type":"null"}]` for nullables.
        if let Some(any_of) = value.items_of("anyOf") {
            let non_null: Vec<&Ordered> = any_of.iter().filter(|b| !b.is_null_schema()).collect();
            return match non_null.len() {
                1 if any_of.len() == 2 => {
                    let inner = self.walk(non_null[0], depth);
                    Ir::Nullable(Box::new(inner))
                }
                0 => Ir::Any,
                1 => self.walk(non_null[0], depth),
                2.. => {
                    let members: Vec<Ir> = non_null.iter().map(|m| self.walk(m, depth)).collect();
                    if members.iter().all(ir_is_stringish)
                        || (members.iter().all(ir_is_discriminated)
                            && union_member_names(&members).is_some_and(|names| {
                                PINNED_UNION_MEMBERS.iter().any(|p| *p == names)
                            }))
                    {
                        Ir::Union(members)
                    } else {
                        Ir::Any
                    }
                }
            };
        }
        if let Some(reference) = value.get("$ref").and_then(Ordered::as_str) {
            let name = reference.rsplit('/').next().unwrap_or_default().to_owned();
            if depth >= REF_DEPTH_CAP || self.in_progress.contains(&name) {
                // Recursive/too-deep schema: accept at the back-edge
                // (never a false reject; counted for the artifact pin).
                self.capped += 1;
                return Ir::Any;
            }
            if let Some(cached) = self.ref_memo.get(&name) {
                return cached.clone();
            }
            let Some(resolved) = self.schemas.get(&name) else {
                self.capped += 1;
                return Ir::Any;
            };
            let resolved = resolved.clone();
            self.in_progress.insert(name.clone());
            let inner = self.walk(&resolved, depth + 1);
            self.in_progress.remove(&name);
            let ir = Ir::Ref { label: name.clone(), node: Box::new(inner) };
            self.ref_memo.insert(name, ir.clone());
            return ir;
        }
        match value.get("type").and_then(Ordered::as_str) {
            Some("object") => self.walk_object(value, depth),
            Some("array") => {
                let items = value.get("items").map(|i| self.walk(i, depth)).unwrap_or(Ir::Any);
                Ir::Arr(Box::new(items))
            }
            Some("string") => match value.items_of("enum") {
                Some(values) => Ir::Literal(
                    values.iter().filter_map(Ordered::as_str).map(str::to_owned).collect(),
                ),
                None => match value.get("pattern").and_then(Ordered::as_str).and_then(plain_prefix)
                {
                    // `^ses` & co: Effect's startsWith filter — pinned
                    // message; complex regexes degrade to plain Str.
                    Some(prefix) => Ir::Pattern { prefix: prefix.to_owned() },
                    None => Ir::Str,
                },
            },
            Some("integer") => Ir::Int {
                minimum: value.get("minimum").and_then(Ordered::as_f64),
                exclusive_minimum: value.get("exclusiveMinimum").and_then(Ordered::as_f64),
            },
            Some("number") => Ir::Num,
            Some("boolean") => Ir::Bool,
            _ => Ir::Any, // allOf/oneOf/not/multi-type — degrade to accept
        }
    }

    fn walk_object(&mut self, value: &Ordered, depth: usize) -> Ir {
        if let Some(properties) = value.get("properties") {
            let required: HashSet<&str> = value
                .items_of("required")
                .map(|list| list.iter().filter_map(Ordered::as_str).collect())
                .unwrap_or_default();
            let mut fields = Vec::new();
            for (name, schema) in properties.members() {
                fields.push(IrField {
                    name: name.clone(),
                    required: required.contains(name.as_str()),
                    ty: self.walk(schema, depth),
                });
            }
            return Ir::Obj { fields };
        }
        // No declared properties: a record (or a plain "anything" object
        // like Session.Metadata). Whole-field mismatches still reject
        // (label "object"); members follow the value node.
        let value_ty = match value.get("additionalProperties") {
            Some(inner) if !matches!(inner, Ordered::Bool(_)) => self.walk(inner, depth),
            _ => Ir::Any,
        };
        Ir::Record(Box::new(value_ty))
    }
}

/// Interns IR into the flat arena, deduping structurally identical nodes
/// by their rendered source line.
struct Arena {
    lines: Vec<String>,
    index: HashMap<String, u32>,
}

impl Arena {
    fn new() -> Self {
        Self { lines: Vec::new(), index: HashMap::new() }
    }

    fn intern(&mut self, ir: &Ir) -> u32 {
        let line = self.render(ir);
        if let Some(existing) = self.index.get(&line) {
            return *existing;
        }
        let id = u32::try_from(self.lines.len()).expect("arena fits u32");
        self.lines.push(line.clone());
        self.index.insert(line, id);
        id
    }

    /// Renders one `SchemaNode` entry (children already interned).
    fn render(&mut self, ir: &Ir) -> String {
        match ir {
            Ir::Any => "SchemaNode::Any".to_owned(),
            Ir::Str => "SchemaNode::Str".to_owned(),
            Ir::Pattern { prefix } => {
                format!("SchemaNode::Pattern {{ prefix: {prefix:?} }}")
            }
            Ir::Num => "SchemaNode::Num".to_owned(),
            Ir::Bool => "SchemaNode::Bool".to_owned(),
            Ir::Int { minimum, exclusive_minimum } => format!(
                "SchemaNode::Int {{ minimum: {}, exclusive_minimum: {} }}",
                fmt_opt_f64(minimum.as_ref()),
                fmt_opt_f64(exclusive_minimum.as_ref())
            ),
            Ir::Literal(values) => format!(
                "SchemaNode::Literal(&[{}])",
                values.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(", ")
            ),
            Ir::Nullable(inner) => format!("SchemaNode::Nullable({})", self.intern(inner)),
            Ir::Arr(inner) => format!("SchemaNode::Arr({})", self.intern(inner)),
            Ir::Record(inner) => format!("SchemaNode::Record({})", self.intern(inner)),
            Ir::Union(members) => {
                let ids: Vec<String> = members.iter().map(|m| self.intern(m).to_string()).collect();
                format!("SchemaNode::Union(&[{}])", ids.join(", "))
            }
            Ir::Ref { label, node } => {
                format!("SchemaNode::Ref {{ label: {label:?}, node: {} }}", self.intern(node))
            }
            Ir::Obj { fields } => {
                let rendered: Vec<String> = fields
                    .iter()
                    .map(|f| {
                        format!(
                            "FieldSpec {{ name: {:?}, required: {}, ty: {} }}",
                            f.name,
                            f.required,
                            self.intern(&f.ty)
                        )
                    })
                    .collect();
                if rendered.is_empty() {
                    "SchemaNode::Obj { fields: &[] }".to_owned()
                } else {
                    format!(
                        "SchemaNode::Obj {{ fields: &[\n    {},\n] }}",
                        rendered.join(",\n    ")
                    )
                }
            }
        }
    }
}

fn fmt_opt_f64(value: Option<&f64>) -> String {
    match value {
        None => "None".to_owned(),
        // Debug formatting keeps float literals well-typed (0.0 not 0).
        Some(v) => format!("Some({v:?})"),
    }
}

/// `^literal` with no regex metacharacters in `literal` → that literal
/// (an Effect `startsWith` filter); every other pattern → `None` (the
/// walker keeps plain `Str` so unknown regexes can never false-reject).
const REGEX_METACHARS: &[char] =
    &['\\', '.', '[', ']', '(', ')', '{', '}', '*', '+', '?', '|', '^', '$', '/'];

fn plain_prefix(pattern: &str) -> Option<&str> {
    let literal = pattern.strip_prefix('^')?;
    (!literal.is_empty() && !literal.contains(REGEX_METACHARS)).then_some(literal)
}

/// One row per operation: the body kind (+ arena index for JSON bodies)
/// and the query-params object node (+ arena index), if any.
struct Row {
    operation_id: String,
    body: String,  // rendered BodyValidation
    query: String, // rendered QueryValidation
}

/// Walks ONE query parameter's schema. Query semantics differ from body
/// semantics in one way that matters for rendering: optional params are
/// `anyOf [X, null]` in the spec, but a URL string is never JSON null —
/// the wrapper is UNWRAPPED (not `Nullable`) so kind-mismatch suffixes
/// come from the interpreter (`| undefined`, probe-pinned), and predicate
/// failures never gain one.
fn walk_query_param(walker: &mut Walker<'_>, schema: &Ordered) -> Ir {
    if let Some(any_of) = schema.items_of("anyOf") {
        let non_null: Vec<&Ordered> = any_of.iter().filter(|b| !b.is_null_schema()).collect();
        if non_null.len() == 1 && any_of.len() == 2 {
            return walker.walk(non_null[0], 0);
        }
    }
    walker.walk(schema, 0)
}

/// The op's query parameters, spec declaration order (Effect reports
/// first-field-wins), as an `Obj` IR — or `None` when it declares none.
/// Path-item-level query parameters are merged first (none exist in the
/// pinned spec; kept for symmetry with `model::parse_operations`).
fn query_ir(root: &Ordered, walker: &mut Walker<'_>, op: &Operation) -> Option<Ir> {
    let path_item = root.get("paths")?.get(&op.path)?;
    let mut schemas: Vec<(String, bool, Ordered)> = Vec::new();
    for source in [path_item.get("parameters"), path_item.get(&op.method)?.get("parameters")] {
        let Some(list) = source else { continue };
        for param in list.items() {
            if param.get("in").and_then(Ordered::as_str) != Some("query") {
                continue;
            }
            let Some(name) = param.get("name").and_then(Ordered::as_str) else { continue };
            if schemas.iter().any(|(existing, _, _)| existing == name) {
                continue;
            }
            let required = matches!(param.get("required"), Some(Ordered::Bool(true)));
            let schema = param.get("schema").cloned().unwrap_or(Ordered::Null);
            schemas.push((name.to_owned(), required, schema));
        }
    }
    if schemas.is_empty() {
        return None;
    }
    let fields = schemas
        .into_iter()
        .map(|(name, required, schema)| IrField {
            name,
            required,
            ty: walk_query_param(walker, &schema),
        })
        .collect();
    Some(Ir::Obj { fields })
}

/// Emits the artifact source. `operations` must be the shared parsed set
/// (same (path, method) sort as the other emitters); `spec_json` is the
/// RAW pinned document (re-parsed order-preserving here).
pub fn emit(spec_json: &str, operations: &[Operation], sha256: &str) -> String {
    let root: Ordered =
        serde_json::from_str(spec_json).expect("pinned openapi.json must be valid JSON");
    let schemas: HashMap<String, Ordered> = root
        .get("components")
        .and_then(|c| c.get("schemas"))
        .map(|s| s.members().iter().cloned().collect())
        .unwrap_or_default();
    let mut walker = Walker {
        schemas: &schemas,
        ref_memo: HashMap::new(),
        in_progress: HashSet::new(),
        capped: 0,
    };
    let mut arena = Arena::new();

    let mut rows: Vec<Row> = Vec::with_capacity(operations.len());
    for op in operations {
        let body = match op_body(&root, op) {
            None => "BodyValidation::None".to_owned(),
            Some(request_body) => {
                let content = request_body.get("content");
                if content.and_then(|c| c.get("application/octet-stream")).is_some() {
                    "BodyValidation::Binary".to_owned()
                } else if let Some(json) =
                    content.and_then(|c| c.get("application/json")).and_then(|j| j.get("schema"))
                {
                    let ir = walker.walk(json, 0);
                    format!("BodyValidation::Json({})", arena.intern(&ir))
                } else {
                    "BodyValidation::None".to_owned()
                }
            }
        };
        let query = match query_ir(&root, &mut walker, op) {
            Some(ir) => format!("QueryValidation::Params({})", arena.intern(&ir)),
            None => "QueryValidation::None".to_owned(),
        };
        rows.push(Row { operation_id: op.operation_id.clone(), body, query });
    }
    rows.sort_by(|a, b| a.operation_id.cmp(&b.operation_id));
    let query_routes = rows.iter().filter(|r| r.query != "QueryValidation::None").count();

    let mut out = String::new();
    out.push_str(crate::emit_routes::ROUTES_HEADER);
    out.push_str(&format!("// Source: tools/openapi-gen/openapi.json (sha256 {sha256})\n"));
    out.push_str(
        "//! Request-validation constraint table for the 141-op HttpApi surface\n\
         //! (HTTP-H0): one row per operation mapping its request body AND its\n\
         //! declared query parameters to JSON-shape constraints. DATA ONLY —\n\
         //! the interpreter/seam lives in `request_validation.rs`, which\n\
         //! enforces a rollout subset (the rest of the table is emitted for\n\
         //! review/coverage but stays pass-through).\n\
         //! Arena: every schema node is one `NODES` entry; object fields are\n\
         //! inline slices; children are indices. Field order is the spec's\n\
         //! DECLARATION order (Effect reports first-field-wins). Unsupported\n\
         //! constructs degrade to `Any` (accept) — counted in CAPPED_NODES,\n\
         //! never a silent false-reject.\n\
         //! Query rows: URL strings are never JSON null, so optional\n\
         //! `anyOf [X, null]` wrappers are UNWRAPPED to X (kind-mismatch\n\
         //! suffixes are interpreter-owned, probe-pinned to\n\
         //! `Expected … | undefined`); NumberFromString integer filters\n\
         //! (limit/context/lines) are decode-time in v2 and absent from the\n\
         //! spec — the seam carries those probe-pinned bounds separately.\n",
    );
    out.push_str("#![allow(clippy::must_use_candidate, clippy::doc_markdown)]\n\n");
    out.push_str(&format!("pub const OPENAPI_SHA256: &str = \"{sha256}\";\n"));
    out.push_str(&format!("pub const OPERATION_COUNT: usize = {};\n", rows.len()));
    out.push_str(&format!(
        "/// Operations declaring at least one query parameter.\n\
         pub const QUERY_PARAM_ROUTES: usize = {query_routes};\n\n"
    ));
    out.push_str(&format!(
        "/// `$ref` back-edges / nests beyond the generator cap ({REF_DEPTH_CAP}) —\n\
         /// emitted as `Any` (accept), counted here so drift is reviewable.\n\
         pub const CAPPED_NODES: usize = {};\n\n",
        walker.capped
    ));
    out.push_str(SCHEMA_TYPES);
    out.push_str("pub static NODES: &[SchemaNode] = &[\n");
    for (i, line) in arena.lines.iter().enumerate() {
        out.push_str(&format!("    {line}, // {i}\n"));
    }
    out.push_str("];\n\n");
    out.push_str("pub static ROUTES: &[RouteValidation] = &[\n");
    for row in &rows {
        out.push_str(&format!(
            "    RouteValidation {{ operation_id: {:?}, body: {}, query: {} }},\n",
            row.operation_id, row.body, row.query
        ));
    }
    out.push_str("];\n\n");
    out.push_str(LOOKUP_AND_TESTS);
    out
}

fn op_body<'a>(root: &'a Ordered, op: &Operation) -> Option<&'a Ordered> {
    root.get("paths")?.get(&op.path)?.get(&op.method)?.get("requestBody")
}

const SCHEMA_TYPES: &str = r#"#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SchemaNode {
    /// Accept anything (unsupported/unmapped construct — never reject).
    Any,
    /// JSON string.
    Str,
    /// JSON string that must start with `prefix` (the spec's `^literal`
    /// patterns — Effect's `startsWith` filter; message-pinned:
    /// `Expected a string starting with "ses"`).
    Pattern {
        prefix: &'static str,
    },
    /// JSON number, integral; `minimum`/`exclusive_minimum` bounds.
    Int {
        minimum: Option<f64>,
        exclusive_minimum: Option<f64>,
    },
    /// JSON number (any).
    Num,
    /// JSON boolean.
    Bool,
    /// Inline string-literal union (`Expected "a" | "b"`).
    Literal(&'static [&'static str]),
    /// Annotated schema ($ref): `label` is the Effect identifier printed
    /// for whole-field mismatches; `node` is the wrapped constraint.
    Ref {
        label: &'static str,
        node: u32,
    },
    /// `anyOf [X, null]` — null passes, else `X` (label gains `| null`).
    Nullable(u32),
    /// JSON array of the item node.
    Arr(u32),
    /// JSON object with declared fields (declaration order preserved).
    Obj {
        fields: &'static [FieldSpec],
    },
    /// JSON object with arbitrary string keys of the value node (also the
    /// plain `{"type":"object"}` shape: value node `Any`).
    Record(u32),
    /// Multi-member `anyOf` without a null member. Only emitted for the
    /// two probe-pinned shapes: string-unions (query filters — the seam
    /// accepts on any member and reports the FIRST member's message) and
    /// discriminator-struct unions (mcp.add `config` — the seam delegates
    /// to the member whose `type` literal matches, else renders the union
    /// label `{ readonly "type": "local", ... } | …`).
    Union(&'static [u32]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    /// Wire name (exact spelling).
    pub name: &'static str,
    /// In the spec's `required` list.
    pub required: bool,
    /// Index into `NODES`.
    pub ty: u32,
}

/// What a route's request body must be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyValidation {
    /// No request body.
    None,
    /// `application/octet-stream` (bytes; not JSON-validated).
    Binary,
    /// `application/json` constrained by `NODES[index]`.
    Json(u32),
}

/// What a route's query string must satisfy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryValidation {
    /// No query parameters declared (unknown keys are stripped upstream).
    None,
    /// Declared parameters as an `Obj` node (`NODES[index]`), spec
    /// declaration order; URL values are strings — see the seam for the
    /// string-semantics interpretation.
    Params(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteValidation {
    pub operation_id: &'static str,
    pub body: BodyValidation,
    pub query: QueryValidation,
}

"#;

const LOOKUP_AND_TESTS: &str = r#"/// Looks a row up by operation id (binary search; `ROUTES` is sorted).
pub fn find(operation_id: &str) -> Option<&'static RouteValidation> {
    ROUTES
        .binary_search_by(|row| row.operation_id.as_bytes().cmp(operation_id.as_bytes()))
        .ok()
        .map(|index| &ROUTES[index])
}

/// Resolves the JSON-body node index for an operation, if it declares one.
pub fn body_node(operation_id: &str) -> Option<u32> {
    match find(operation_id)?.body {
        BodyValidation::Json(node) => Some(node),
        _ => None,
    }
}

/// Resolves the query-params `Obj` node index for an operation, if it
/// declares any query parameters.
pub fn query_node(operation_id: &str) -> Option<u32> {
    match find(operation_id)?.query {
        QueryValidation::Params(node) => Some(node),
        QueryValidation::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every generated route must have exactly one validation row.
    #[test]
    fn covers_all_operations() {
        // No schema degraded to `Any` by the generator caps (oracle
        // condition 3) — any drift here must be reviewed with the
        // regeneration commit.
        assert_eq!(CAPPED_NODES, 0);
        assert_eq!(ROUTES.len(), OPERATION_COUNT);
        assert_eq!(ROUTES.len(), crate::routes_gen::ROUTES.len());
        let table: std::collections::BTreeSet<&str> =
            ROUTES.iter().map(|r| r.operation_id).collect();
        let routes: std::collections::BTreeSet<&str> =
            crate::routes_gen::ROUTES.iter().map(|r| r.operation_id).collect();
        assert_eq!(table, routes, "validation rows must mirror routes.gen.rs");
    }

    #[test]
    fn rows_sorted_for_binary_search() {
        assert!(ROUTES.windows(2).all(|w| w[0].operation_id < w[1].operation_id));
        assert!(find("shell.create").is_some());
        assert!(find("no.such.op").is_none());
    }

    /// All child indices (and field ty indices) must point into NODES.
    #[test]
    fn arena_indices_in_range() {
        fn visit(node: u32, seen: &mut std::collections::BTreeSet<u32>) {
            assert!((node as usize) < NODES.len(), "node index {node} out of range");
            if !seen.insert(node) {
                return; // shared subgraph
            }
            match &NODES[node as usize] {
                SchemaNode::Ref { node, .. }
                | SchemaNode::Nullable(node)
                | SchemaNode::Arr(node)
                | SchemaNode::Record(node) => visit(*node, seen),
                SchemaNode::Union(members) => {
                    for member in *members {
                        visit(*member, seen);
                    }
                }
                SchemaNode::Obj { fields } => {
                    for field in *fields {
                        assert!(
                            (field.ty as usize) < NODES.len(),
                            "field {} ty out of range",
                            field.name
                        );
                        visit(field.ty, seen);
                    }
                }
                _ => {}
            }
        }
        for row in ROUTES {
            if let BodyValidation::Json(node) = row.body {
                visit(node, &mut std::collections::BTreeSet::new());
            }
            if let QueryValidation::Params(node) = row.query {
                visit(node, &mut std::collections::BTreeSet::new());
            }
        }
    }

    /// Slice pins: constraint shapes this lane's seam enforces, frozen
    /// against accidental regeneration drift.
    #[test]
    fn slice_pins() {
        fn fields(node: u32) -> &'static [FieldSpec] {
            match &NODES[node as usize] {
                SchemaNode::Obj { fields } => fields,
                other => panic!("node {node} is not an Obj: {other:?}"),
            }
        }
        fn field_of(op: &str, field: &str) -> &'static FieldSpec {
            let root = body_node(op).unwrap_or_else(|| panic!("{op} has no JSON body"));
            fields(root)
                .iter()
                .find(|f| f.name == field)
                .unwrap_or_else(|| panic!("{op} has no field {field}"))
        }

        // shell.create: command required string; timeout Int minimum 0.
        assert_eq!(NODES[field_of("shell.create", "command").ty as usize], SchemaNode::Str);
        assert!(field_of("shell.create", "command").required);
        assert_eq!(
            NODES[field_of("shell.create", "timeout").ty as usize],
            SchemaNode::Int { minimum: Some(0.0), exclusive_minimum: None }
        );

        // plugin.update: targets required array-of-string.
        assert_eq!(
            NODES[field_of("plugin.update", "targets").ty as usize],
            SchemaNode::Arr(
                u32::try_from(NODES.iter().position(|n| matches!(n, SchemaNode::Str)).unwrap())
                    .unwrap()
            )
        );
        assert!(field_of("plugin.update", "targets").required);

        // session.move: directory required; delivery Nullable(Ref labeled).
        assert!(field_of("session.move", "directory").required);
        let delivery = field_of("session.move", "delivery").ty;
        match &NODES[delivery as usize] {
            SchemaNode::Nullable(inner) => match &NODES[*inner as usize] {
                SchemaNode::Ref { label, .. } => assert_eq!(*label, "Session.Inbox.Delivery"),
                other => panic!("delivery inner not a Ref: {other:?}"),
            },
            other => panic!("delivery not Nullable: {other:?}"),
        }

        // pty.update: inline size struct, both members required integers.
        let size = field_of("pty.update", "size").ty;
        let size_fields = fields(size);
        assert_eq!(size_fields.len(), 2);
        assert!(size_fields.iter().all(|f| f.required));

        // session.create: title nullable string (Effect `| null` label);
        // id/parentID nullable `^ses` startsWith patterns.
        match &NODES[field_of("session.create", "title").ty as usize] {
            SchemaNode::Nullable(inner) => assert_eq!(NODES[*inner as usize], SchemaNode::Str),
            other => panic!("title not nullable: {other:?}"),
        }
        for pattern_field in ["id", "parentID"] {
            match &NODES[field_of("session.create", pattern_field).ty as usize] {
                SchemaNode::Nullable(inner) => assert_eq!(
                    NODES[*inner as usize],
                    SchemaNode::Pattern { prefix: "ses" },
                    "session.create {pattern_field} must be a nullable ^ses pattern"
                ),
                other => panic!("{pattern_field} not nullable: {other:?}"),
            }
        }

        // experimental.mcp.add: required config as a discriminator-struct
        // union (probe-pinned members local/remote).
        match &NODES[field_of("experimental.mcp.add", "config").ty as usize] {
            SchemaNode::Union(members) => {
                assert_eq!(members.len(), 2, "config union members: {members:?}");
                for member in *members {
                    match &NODES[*member as usize] {
                        SchemaNode::Ref { node, .. } => {
                            let type_field = fields(*node)
                                .iter()
                                .find(|f| f.name == "type")
                                .unwrap_or_else(|| panic!("union member has no type field"));
                            match &NODES[type_field.ty as usize] {
                                SchemaNode::Literal(values) => assert_eq!(values.len(), 1),
                                other => panic!("type field not a literal: {other:?}"),
                            }
                        }
                        other => panic!("config union member not a Ref: {other:?}"),
                    }
                }
            }
            other => panic!("config not a Union: {other:?}"),
        }

        // rpc.call: root is the annotated Rpc.Input (probe: whole-root
        // mismatch prints the identifier, not `Expected object`).
        match &NODES[body_node("rpc.call").unwrap() as usize] {
            SchemaNode::Ref { label, .. } => assert_eq!(*label, "Rpc.Input"),
            other => panic!("rpc.call root not a Ref: {other:?}"),
        }

        // Routes without JSON bodies stay pass-through.
        assert_eq!(find("session.list").map(|r| r.body), Some(BodyValidation::None));
        assert_eq!(find("fs.read").map(|r| r.body), Some(BodyValidation::None));
    }

    /// Query-table pins: declaration-order fields and the constraint
    /// classes the seam interprets (probe evidence rollout-probe*.json).
    #[test]
    fn query_slice_pins() {
        fn params(op: &str) -> &'static [FieldSpec] {
            let node = query_node(op).unwrap_or_else(|| panic!("{op} has no query row"));
            match &NODES[node as usize] {
                SchemaNode::Obj { fields } => fields,
                other => panic!("{op} query node not an Obj: {other:?}"),
            }
        }
        fn param_of(op: &str, name: &str) -> &'static FieldSpec {
            params(op)
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{op} has no query param {name}"))
        }

        // Count: 71 ops declare query parameters (QUERY_PARAM_ROUTES).
        assert_eq!(
            ROUTES.iter().filter(|r| matches!(r.query, QueryValidation::Params(_))).count(),
            QUERY_PARAM_ROUTES
        );

        // session.list: limit/order/search/parentID… in declaration order;
        // limit is a plain string in the spec (the NumberFromString
        // integer filter is seam-side, probe-pinned); order a literal
        // union; parentID the string-union [^ses, "null"].
        let names: Vec<&str> = params("session.list").iter().map(|f| f.name).collect();
        assert_eq!(
            names,
            ["limit", "order", "search", "parentID", "directory", "project", "subpath", "cursor"]
        );
        assert_eq!(NODES[param_of("session.list", "limit").ty as usize], SchemaNode::Str);
        match &NODES[param_of("session.list", "order").ty as usize] {
            SchemaNode::Literal(values) => {
                assert_eq!(values, &["asc", "desc"]);
            }
            other => panic!("order not a Literal: {other:?}"),
        }
        match &NODES[param_of("session.list", "parentID").ty as usize] {
            SchemaNode::Union(members) => {
                assert_eq!(members.len(), 2);
                match &NODES[members[0] as usize] {
                    SchemaNode::Pattern { prefix } => assert_eq!(*prefix, "ses"),
                    other => panic!("parentID first member not ^ses: {other:?}"),
                }
                match &NODES[members[1] as usize] {
                    SchemaNode::Literal(values) => assert_eq!(values, &["null"]),
                    other => panic!("parentID second member not \"null\": {other:?}"),
                }
            }
            other => panic!("parentID not a Union: {other:?}"),
        }

        // vcs.diff: REQUIRED mode as a $ref'd literal union (probe: whole
        // mismatch prints the identifier `Expected Vcs.Mode`).
        let mode = param_of("vcs.diff", "mode");
        assert!(mode.required);
        match &NODES[mode.ty as usize] {
            SchemaNode::Ref { label, node } => {
                assert_eq!(*label, "Vcs.Mode");
                match &NODES[*node as usize] {
                    SchemaNode::Literal(values) => {
                        assert_eq!(values, &["working", "branch", "committed"]);
                    }
                    other => panic!("Vcs.Mode not a Literal: {other:?}"),
                }
            }
            other => panic!("mode not a Ref: {other:?}"),
        }

        // fs.find: required `query` string; `type` file/directory union.
        assert!(param_of("fs.find", "query").required);
        match &NODES[param_of("fs.find", "type").ty as usize] {
            SchemaNode::Literal(values) => assert_eq!(values, &["file", "directory"]),
            other => panic!("fs.find type not a Literal: {other:?}"),
        }

        // session.diff: `from`/`to` are `^msg_` patterns.
        for name in ["from", "to"] {
            match &NODES[param_of("session.diff", name).ty as usize] {
                SchemaNode::Pattern { prefix } => assert_eq!(*prefix, "msg_"),
                other => panic!("session.diff {name} not a ^msg_ pattern: {other:?}"),
            }
        }

        // worktree.list / experimental.fs.write: required plain strings.
        assert!(param_of("worktree.list", "projectID").required);
        assert!(param_of("experimental.fs.write", "path").required);
        assert!(query_node("session.list").is_some());
        assert_eq!(query_node("session.create"), None, "session.create declares no query");
        // pty.connect's raw flat params (location[directory]/cursor/ticket)
        // are plain strings — nothing can fail.
        for name in ["location[directory]", "cursor", "ticket"] {
            assert_eq!(
                NODES[param_of("pty.connect", name).ty as usize],
                SchemaNode::Str,
                "pty.connect {name}"
            );
        }
    }
}
"#;
