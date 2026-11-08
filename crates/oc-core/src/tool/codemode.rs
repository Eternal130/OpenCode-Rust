//! Code Mode (`packages/core/src/codemode/*` + the tool.ts snapshot
//! split): codeMode-eligible tools become **invisible-direct** — hidden
//! from the request's tool definitions and callable only through one
//! synthesized `execute` tool whose input is a JavaScript program.
//!
//! Port shape (core-runner-map §4.1):
//!
//! * [`shape_request_tools`] — the `Tool.snapshot` split
//!   (tool.ts:225-241): wholly-denied tools drop (upstream), direct
//!   tools (`options.codemode === false`) keep their definitions, every
//!   other tool moves behind `execute`, and `execute` itself is
//!   advertised unless the `"execute"` action is wholly denied.
//! * [`CatalogEntry`] + [`catalog_summary`] + [`render_instructions`] —
//!   the catalog (`codemode/catalog.ts` `summarize` + the inline-budget
//!   round-robin) and its instruction text (`codemode/instructions.ts`
//!   `render`, byte-pinned against v2.0.24).
//! * [`signature`] — `toolExpression(path)(input): Promise<output>` —
//!   the JSON-Schema→TypeScript renderer (`codemode/tool-schema.ts`,
//!   pretty form). Registrations carry no output schemas, so outputs
//!   render `void` exactly like a schema-less v2 tool.
//! * [`CodeModeRuntime`] — the interpreter seam. v2 runs the program on
//!   the in-process confined interpreter (`@opencode/codemode`, ~6.6k
//!   LOC); the Rust port runs it in the plugin host's guest runtime
//!   (oc-plugin-host `codemode.evaluate`), so the trait is the boundary
//!   the integrator implements. [`register_code_mode`] wires the
//!   executor behind the standard pipeline (before/after hooks + decode
//!   fire for `execute` AND every nested call, like v2's
//!   `executeTool` wrapping).
//!
//! Divergences (documented, wave 4):
//! * `search`-over-inventory IS available in the guest runtime: the
//!   host renders each entry's ref-shaped `searchText` (names +
//!   descriptions + input property names/descriptions,
//!   [`search_text`]) and the guest runs the v2-ranked `search`
//!   builtin (tokenize/termForms/rank, default limit 10) over it.
//! * Nested results prefer the tool's declared structured output,
//!   else the joined text of the content parts, else null (v2
//!   codemode/tool.ts:101-106 via [`CodeModeLeaf::nested_value`]).
//! * Confined-interpreter guarantees map onto the guest sandbox: the
//!   catalog allowlist stands in for the interpreter's globals, and
//!   the v2 `ExecutionLimits` (codemode.ts:22-41) are at parity as an
//!   OPT-IN at this seam — [`register_code_mode_with_limits`] /
//!   [`ExecutionLimits`] (absent = unlimited, exactly v2's core
//!   callsites, which pass no limits). `timeoutMs` is layered: the
//!   guest enforces it between event-loop turns (async hangs), the
//!   supervisor kills a wedged (sync-loop) guest at the transport
//!   deadline; `maxToolCalls` is the admission counter on both sides;
//!   `maxOutputBytes` bounds the value at the guest boundary and
//!   warnings/logs at the leaf ([`bound_output`]); `MAX_VALUE_DEPTH`
//!   guards every value crossing guest↔host. The ref's other
//!   structural caps (MAX_STRING_LENGTH/MAX_ARRAY_LENGTH/
//!   MAX_PENDING_PROMISES, interpreter/limits.ts) are enforced by the
//!   guest on the host↔guest bridge: every string/array crossing
//!   (tool inputs, tool results, the return value) is checked with
//!   the ref's exact messages, and a pending-request counter rejects
//!   the (N+1)-th concurrent bridge promise with the ref's
//!   `Too many pending promises` error.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::location::{ToolCallContext, ToolRegistration, ToolRegistryState, ToolRunResult};
use oc_schema::permission::Ruleset;
use oc_schema::tool::Content;

use super::registry::{self, normalized_name};

/// The synthesized tool's name (reserved: a direct tool may not take it).
pub const EXECUTE_NAME: &str = "execute";

/// The invariant description (`codemode/tool.ts:62-69`) — the catalog
/// itself rides the instructions, not the definition.
pub const EXECUTE_DESCRIPTION: &str = "Run JavaScript in a confined Code Mode runtime to script tool calls and HTTP requests and compose their results.\n\
`fetch` is available for HTTP requests. Imports, direct filesystem access, and timers are unavailable; all other external access goes through `tools`.\n\
Within `{ code }`, the only callable tools are those explicitly listed in the Code Mode catalog instructions or returned by the `search` function. Inside `{ code }`, ignore tools shown outside the Code Mode catalog. They are not available in the Code Mode runtime.\n\
Call tools through `tools` using only exact paths and signatures from the catalog. Do not infer or normalize tool names; preserve bracket notation such as `tools.<namespace>[\"tool-name\"](input)`.\n\
Prefer an explicit `return`; if omitted, the final top-level expression becomes the result.\n\
Await every call whose completion matters; pending calls are interrupted when execution ends. Run independent calls concurrently with `Promise.allSettled`.";

/// `CodeMode.Input` (`codemode/codemode.ts:65`).
pub fn execute_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "code": { "type": "string" } },
        "required": ["code"],
    })
}

/// The `execute` registration row (direct: `codemode: false`; the
/// permission action is the bare name `execute`).
#[must_use]
pub fn execute_registration() -> ToolRegistration {
    let mut registration =
        ToolRegistration::new(EXECUTE_NAME, EXECUTE_DESCRIPTION, execute_input_schema());
    registration.codemode = false;
    registration
}

/// `Tool.snapshot` split (tool.ts:225-241). `code_mode_registered` —
/// whether an `execute` executor is installed (an engine without a
/// [`CodeModeRuntime`] keeps today's flat behavior: every tool direct).
/// When active, the request carries the direct tools plus `execute`
/// LAST (definitions sort by name, the CodeMode tool appended).
#[must_use]
pub fn shape_request_tools(
    snapshot: Vec<ToolRegistration>,
    permissions: Option<&Ruleset>,
    code_mode_registered: bool,
) -> Vec<ToolRegistration> {
    static EMPTY: Ruleset = Vec::new();
    let rules = permissions.unwrap_or(&EMPTY);
    if !code_mode_registered || crate::location::wholly_denied(EXECUTE_NAME, rules) {
        return snapshot;
    }
    let mut tools: Vec<ToolRegistration> =
        snapshot.into_iter().filter(|tool| !tool.codemode && tool.name != EXECUTE_NAME).collect();
    tools.push(execute_registration());
    tools
}

// ---------------------------------------------------------------------------
// Catalog entries + signatures (tool-runtime.ts describeTool)
// ---------------------------------------------------------------------------

/// One code-mode tool as the catalog and the runtime see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// The qualified dotted path (`namespace.name` / `name`).
    pub path: String,
    pub description: String,
    /// `toolExpression(path)(input): Promise<output>`.
    pub signature: String,
    /// `toSearchEntry.searchText` (tool-runtime.ts:289-299): path +
    /// description + input property names/descriptions, lowercased —
    /// the lowest-tier match surface of the `search` builtin (our
    /// namespaces carry no descriptions, exactly the v2 rows here).
    pub search_text: String,
    /// `options.pinned` — pinned listings are always shown in the summary
    /// (prepaid against the inline budget, excluded from the round-robin;
    /// catalog.ts:79-90). The sole v2 registration with `pinned: true` is
    /// `opencode.session_move` (opencode.ts:115).
    pub pinned: bool,
    /// G-106② — the registered description of the entry's namespace
    /// (`editor.namespace(...)`, ref tool.ts:169-176), when one exists.
    /// All entries of a namespace share it; the catalog renders it on
    /// the namespace line (catalog.ts:64/173).
    pub namespace_description: Option<String>,
}

/// `qualifiedName` (codemode/tool.ts:296-300) over a registration.
#[must_use]
pub fn qualified_path(registration: &ToolRegistration) -> String {
    let normalized = normalized_name(&registration.name);
    match &registration.namespace {
        Some(namespace) => format!("{namespace}.{normalized}"),
        None => normalized,
    }
}

/// `toSearchEntry.searchText` (tool-runtime.ts:289-299): path +
/// description + input property names and their descriptions, joined
/// with newlines and lowercased (the ref also folds namespace
/// descriptions in — our namespaces carry none).
#[must_use]
pub fn search_text(path: &str, description: &str, input: &Value) -> String {
    let mut parts = vec![path.to_string(), description.to_string()];
    if let Some(properties) = input.get("properties").and_then(Value::as_object) {
        for (name, property) in properties {
            parts.push(name.clone());
            if let Some(text) = property.get("description").and_then(Value::as_str) {
                parts.push(text.to_string());
            }
        }
    }
    parts.join("\n").to_lowercase()
}

/// The code-mode half of a registry snapshot (permission filtering is
/// the caller's; the catalog mirrors the request's tool set).
#[must_use]
pub fn code_mode_entries(tools: &ToolRegistryState) -> Vec<CatalogEntry> {
    tools
        .snapshot(None)
        .iter()
        .filter(|tool| tool.codemode)
        .map(|tool| {
            let path = qualified_path(tool);
            let pinned = path == "opencode.session_move";
            let signature = signature(&path, &tool.description, &tool.input);
            let search_text = search_text(&path, &tool.description, &tool.input);
            let namespace_description =
                tools.namespace_description(path.split('.').next().unwrap_or(&path));
            CatalogEntry {
                pinned,
                path,
                description: tool.description.clone(),
                signature,
                search_text,
                namespace_description,
            }
        })
        .collect()
}

/// `identifierSegment` (`/^[A-Za-z_$][A-Za-z0-9_$]*$/`).
fn identifier_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' || first == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// `toolExpression` (tool-runtime.ts:108-113): `tools` + dotted or
/// bracket-quoted segments.
#[must_use]
pub fn tool_expression(path: &str) -> String {
    let mut out = String::from("tools");
    for segment in path.split('.') {
        if identifier_segment(segment) {
            out.push('.');
            out.push_str(segment);
        } else {
            out.push('[');
            out.push_str(
                &serde_json::to_string(segment).unwrap_or_else(|_| format!("{segment:?}")),
            );
            out.push(']');
        }
    }
    out
}

/// `describeTool.signature` (tool-runtime.ts:171-183). Outputs are
/// always `void` — registrations carry no output schemas (a schema-less
/// v2 tool renders identically).
#[must_use]
pub fn signature(path: &str, _description: &str, input: &Value) -> String {
    let rendered = render_schema(input, 0, true);
    format!(
        "{}{}: Promise<void>",
        tool_expression(path),
        if rendered == "{}" { "()".to_string() } else { format!("({rendered})") }
    )
}

// ---------------------------------------------------------------------------
// JSON-Schema → TypeScript (tool-schema.ts renderSchema, pretty form)
// ---------------------------------------------------------------------------

const MAX_RENDER_DEPTH: usize = 8;

fn render_key(name: &str) -> String {
    if identifier_segment(name) {
        name.to_string()
    } else {
        serde_json::to_string(name).unwrap_or_else(|_| format!("{name:?}"))
    }
}

fn render_literal(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "unknown".to_string())
}

/// `docTags` (tool-schema.ts:56-79) — exact order.
fn doc_tags(schema: &Value) -> Vec<String> {
    let object = schema.as_object();
    let get = |key: &str| object.and_then(|map| map.get(key));
    let mut tags = Vec::new();
    if get("deprecated").and_then(Value::as_bool) == Some(true) {
        tags.push("@deprecated".to_string());
    }
    if let Some(default) = get("default") {
        if let Ok(rendered) = serde_json::to_string(default) {
            tags.push(format!("@default {rendered}"));
        }
    }
    if let Some(format) = get("format").and_then(Value::as_str) {
        tags.push(format!("@format {format}"));
    }
    if get("type").and_then(Value::as_str) == Some("integer") {
        tags.push("@integer".to_string());
    }
    for (tag, key) in [
        ("@minimum", "minimum"),
        ("@maximum", "maximum"),
        ("@exclusiveMinimum", "exclusiveMinimum"),
        ("@exclusiveMaximum", "exclusiveMaximum"),
        ("@multipleOf", "multipleOf"),
        ("@minLength", "minLength"),
        ("@maxLength", "maxLength"),
    ] {
        if let Some(number) = get(key).and_then(Value::as_f64) {
            tags.push(format!("{tag} {}", trim_number(number)));
        }
    }
    if let Some(pattern) = get("pattern").and_then(Value::as_str) {
        tags.push(format!("@pattern {pattern}"));
    }
    for (tag, key) in [("@minItems", "minItems"), ("@maxItems", "maxItems")] {
        if let Some(number) = get(key).and_then(Value::as_f64) {
            tags.push(format!("{tag} {}", trim_number(number)));
        }
    }
    if get("uniqueItems").and_then(Value::as_bool) == Some(true) {
        tags.push("@uniqueItems true".to_string());
    }
    tags
}

/// JSON.stringify's number form (integral values without a tail).
fn trim_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// `docLines` (tool-schema.ts:81-90) — trimmed description lines; a
/// single-line description inlines with the tag summary.
fn doc_lines(schema: &Value, width: usize) -> Vec<String> {
    let summary = doc_tags(schema).join(" ");
    let mut lines: Vec<String> = schema
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split('\n')
        .map(|line| line.trim_end().to_string())
        .collect();
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    if !summary.is_empty()
        && lines.len() == 1
        && !summary.contains('\n')
        && width + summary.len() + 7 <= 120
    {
        return vec![summary];
    }
    let mut out = lines;
    if !summary.is_empty() {
        out.extend(summary.split('\n').map(str::to_string));
    }
    out
}

/// `jsdoc` (tool-schema.ts:93-119) — one `/** … */` block for a
/// property's immediate docs.
fn jsdoc(schema: &Value, pad: &str) -> String {
    let content = doc_lines(schema, pad.len());
    if content.is_empty() {
        return String::new();
    }
    if content.len() == 1 && pad.len() + content[0].len() + 7 <= 120 {
        return format!("{pad}/** {} */\n", content[0]);
    }
    let body = content
        .iter()
        .map(|line| if line.is_empty() { format!("{pad} *") } else { format!("{pad} * {line}") })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{pad}/**\n{body}\n{pad} */\n")
}

/// `renderSchema` (tool-schema.ts:121-201), pretty form. The port
/// covers the keyword subset our registrations emit; `$ref` renders
/// `unknown` (no `$defs` plumbing — inline schemas only).
fn render_schema(schema: &Value, depth: usize, pretty: bool) -> String {
    if depth > MAX_RENDER_DEPTH {
        return "unknown".to_string();
    }
    let Some(object) = schema.as_object() else {
        return "unknown".to_string();
    };
    if let Some(constant) = object.get("const") {
        return render_literal(constant);
    }
    if let Some(enum_values) = object.get("enum").and_then(Value::as_array) {
        return enum_values.iter().map(render_literal).collect::<Vec<_>>().join(" | ");
    }
    if let Some(alternatives) =
        object.get("anyOf").or_else(|| object.get("oneOf")).and_then(Value::as_array)
    {
        // Effect's number sentinel renders plain `number`.
        let number_sentinel = |item: &Value| {
            item.get("type").and_then(Value::as_str) == Some("string")
                && item.get("enum").and_then(Value::as_array).is_some_and(|values| {
                    values.len() == 1
                        && matches!(
                            values[0].as_str(),
                            Some("NaN") | Some("Infinity") | Some("-Infinity")
                        )
                })
        };
        let all_numbers = alternatives.iter().all(|item| {
            item.get("type").and_then(Value::as_str) == Some("number") || number_sentinel(item)
        });
        if all_numbers
            && alternatives
                .iter()
                .any(|item| item.get("type").and_then(Value::as_str) == Some("number"))
        {
            return "number".to_string();
        }
        let members: Vec<String> =
            alternatives.iter().map(|item| render_schema(item, depth + 1, pretty)).collect();
        if members.iter().any(|member| member == "unknown") {
            return "unknown".to_string();
        }
        return members.join(" | ");
    }
    if let Some(members) = object.get("allOf").and_then(Value::as_array) {
        let rendered: Vec<String> =
            members.iter().map(|item| render_schema(item, depth + 1, pretty)).collect();
        let concrete: Vec<String> =
            rendered.into_iter().filter(|member| member != "unknown").collect();
        if concrete.is_empty() {
            return "unknown".to_string();
        }
        if concrete.len() == 1 {
            return concrete.into_iter().next().unwrap_or_default();
        }
        return concrete
            .into_iter()
            .map(|member| if member.contains(" | ") { format!("({member})") } else { member })
            .collect::<Vec<_>>()
            .join(" & ");
    }
    if let Some(types) = object.get("type").and_then(Value::as_array) {
        return types
            .iter()
            .map(|item| {
                let mut single = serde_json::Map::clone(object);
                single.insert("type".to_string(), item.clone());
                render_schema(&Value::Object(single), depth + 1, pretty)
            })
            .collect::<Vec<_>>()
            .join(" | ");
    }
    match object.get("type").and_then(Value::as_str) {
        Some("string") => "string".to_string(),
        Some("number") | Some("integer") => "number".to_string(),
        Some("boolean") => "boolean".to_string(),
        Some("null") => "null".to_string(),
        Some("array") => {
            let items = object.get("items").cloned().unwrap_or(Value::Null);
            format!("Array<{}>", render_schema(&items, depth + 1, pretty))
        }
        Some("object") | None => {
            let properties: Vec<(String, &Value)> = object
                .get("properties")
                .and_then(Value::as_object)
                .map(|map| map.iter().map(|(key, value)| (key.clone(), value)).collect())
                .unwrap_or_default();
            if properties.is_empty() && object.get("type").is_some() {
                return "{}".to_string();
            }
            if properties.is_empty() {
                return "unknown".to_string();
            }
            let required: Vec<&str> = object
                .get("required")
                .and_then(Value::as_array)
                .map(|values| values.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            render_object(&properties, &required, depth, pretty)
        }
        _ => "unknown".to_string(),
    }
}

/// The object arm with an explicit field order. `serde_json` maps sort
/// alphabetically (no `preserve_order`), so general schemas render
/// sorted; the `search` builtin's own schemas pass v2's declaration
/// order to keep the pinned signature byte-exact.
fn render_object(
    fields: &[(String, &Value)],
    required: &[&str],
    depth: usize,
    pretty: bool,
) -> String {
    let field = |(name, value): &(String, &Value)| {
        let optional = if required.contains(&name.as_str()) { "" } else { "?" };
        format!("{}{}: {}", render_key(name), optional, render_schema(value, depth + 1, pretty))
    };
    if !pretty {
        let rendered: Vec<String> = fields.iter().map(|pair| field(pair)).collect();
        return if rendered.is_empty() {
            "{}".to_string()
        } else {
            format!("{{ {} }}", rendered.join("; "))
        };
    }
    let pad = "  ".repeat(depth + 1);
    let lines: Vec<String> = fields
        .iter()
        .map(|(name, value)| {
            format!("{}{}{},", jsdoc(value, &pad), pad, field(&(name.clone(), *value)))
        })
        .collect();
    format!("{{\n{}\n{}}}", lines.join("\n"), "  ".repeat(depth))
}

// ---------------------------------------------------------------------------
// The `search` builtin's schemas + exact signature (tool-runtime.ts
// SearchInput/SearchOutput + searchSignature)
// ---------------------------------------------------------------------------

/// `searchSignature` — rendered from the builtin's own schemas in v2's
/// declaration order (property maps in the JSON path sort
/// alphabetically; the search schemas keep the pinned byte form).
#[must_use]
pub fn search_signature() -> String {
    let string_schema = json!({ "type": "string" });
    let query = string_schema.clone();
    let namespace = string_schema.clone();
    let limit = json!({ "type": "integer", "exclusiveMinimum": 0 });
    let offset = json!({ "type": "integer", "minimum": 0 });
    let input_fields: Vec<(String, &Value)> = vec![
        ("query".to_string(), &query),
        ("namespace".to_string(), &namespace),
        ("limit".to_string(), &limit),
        ("offset".to_string(), &offset),
    ];
    // Output members at v2's declaration depth (fields at depth 1 nest
    // their members at depth 2 — see render_object's pad rule).
    let item_fields: Vec<(String, &Value)> = vec![
        ("path".to_string(), &string_schema),
        ("description".to_string(), &query),
        ("signature".to_string(), &namespace),
    ];
    let item_object = render_object(&item_fields, &["path", "description", "signature"], 2, true);
    let offset_schema = json!({ "type": "integer", "minimum": 0 });
    let next_fields: Vec<(String, &Value)> = vec![("offset".to_string(), &offset_schema)];
    let next_member = render_object(&next_fields, &["offset"], 2, true);
    // Compose the output object with pre-rendered members (all fields
    // required — Schema.Struct).
    let lines = [
        format!("  items: Array<{item_object}>,"),
        format!(
            "  /** @integer @minimum 0 */\n  remaining: {},",
            render_schema(&offset_schema, 1, true)
        ),
        format!("  next: {next_member} | null,"),
    ];
    format!("search({}): {{\n{}\n}}", render_object(&input_fields, &[], 0, true), lines.join("\n"))
}

// ---------------------------------------------------------------------------
// Catalog summarize + instruction render (catalog.ts + instructions.ts)
// ---------------------------------------------------------------------------

const DESCRIPTION_LIMIT: usize = 120;
const CHARACTERS_PER_TOKEN: usize = 4;
const INLINE_BUDGET: usize = 2_000;

/// One namespace group of the summary.
#[derive(Debug, Clone, PartialEq)]
pub struct NamespaceSummary {
    pub name: String,
    pub description: Option<String>,
    pub count: usize,
    pub entries: Vec<Listing>,
}

/// One selected catalog listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub path: String,
    pub line: String,
}

/// `CodeModeCatalog.Summary`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CatalogSummary {
    pub total: usize,
    pub shown: usize,
    pub namespaces: Vec<NamespaceSummary>,
}

fn cost(text: &str) -> usize {
    // G-106④ — ref catalog.ts:188 counts UTF-16 code units
    // (`text.length`) and rounds (`Math.round(units / 4)`). Byte-based
    // counting overestimated non-ASCII content 1-3x and starved the
    // round-robin early.
    (text.encode_utf16().count() + CHARACTERS_PER_TOKEN / 2) / CHARACTERS_PER_TOKEN
}

/// JS `String.prototype.slice(0, units)` (UTF-16 code units). A cut
/// inside a surrogate pair yields a replacement character, exactly like
/// the JS slice does.
fn utf16_slice(value: &str, units: usize) -> String {
    if value.encode_utf16().count() <= units {
        return value.to_string();
    }
    String::from_utf16_lossy(&value.encode_utf16().take(units).collect::<Vec<u16>>())
}

/// The listing suffix: `  - {signature} // {first description line}`.
fn listing_line(entry: &CatalogEntry) -> String {
    let first_line = entry.description.split('\n').next().unwrap_or_default().trim().to_string();
    // G-106③ — the limit and the slice are UTF-16 code units (ref
    // catalog.ts:69 `firstLine.slice(0, DESCRIPTION_LIMIT - 3)`); the
    // previous byte slice panicked on multibyte boundaries.
    let description = if first_line.encode_utf16().count() > DESCRIPTION_LIMIT {
        format!("{}...", utf16_slice(&first_line, DESCRIPTION_LIMIT - 3))
    } else {
        first_line
    };
    let suffix = if description.is_empty() { String::new() } else { format!(" // {description}") };
    format!("  - {}{}", entry.signature, suffix)
}

/// `CodeModeCatalog.summarize` (catalog.ts:54-140): group by the first
/// path segment (sorted), then select full listings shortest-first, one
/// per namespace per round, until the inline budget is spent. Pinned
/// listings (`options.pinned`, e.g. `opencode.session_move`) are always
/// selected — prepaid against the budget, excluded from the round-robin
/// (catalog.ts:79-90, 113-120).
#[must_use]
pub fn catalog_summary(entries: &[CatalogEntry]) -> CatalogSummary {
    // Group by the first path segment (Map.groupBy over split(".", 1)).
    let mut groups: BTreeMap<String, Vec<&CatalogEntry>> = BTreeMap::new();
    for entry in entries {
        let group = entry.path.split('.').next().unwrap_or(&entry.path).to_string();
        groups.entry(group).or_default().push(entry);
    }
    struct Round {
        name: String,
        description: Option<String>,
        listings: Vec<Listing>,
        ranked: Vec<usize>, // listing indexes, cost-ascending then path
        selected: Vec<usize>,
        index: usize,
    }
    let mut rounds: Vec<Round> = groups
        .into_iter()
        .map(|(name, group)| {
            // G-106② — the namespace's registered description (ref
            // catalog.ts:64 `flattened.namespaces.get(name)?.description`).
            let description = group.iter().find_map(|entry| entry.namespace_description.clone());
            // Listings sorted by path inside the namespace.
            let pinned_paths: std::collections::HashSet<&str> = group
                .iter()
                .filter(|entry| entry.pinned)
                .map(|entry| entry.path.as_str())
                .collect();
            let mut listings: Vec<Listing> = group
                .iter()
                .map(|entry| Listing { path: entry.path.clone(), line: listing_line(entry) })
                .collect();
            listings.sort_by(|left, right| left.path.cmp(&right.path));
            // Pinned listings are always selected (catalog.ts:84-90): seed
            // them into `selected` and drop them from the rotation.
            let pinned: Vec<usize> = listings
                .iter()
                .enumerate()
                .filter(|(_, listing)| pinned_paths.contains(listing.path.as_str()))
                .map(|(at, _)| at)
                .collect();
            let selected = pinned.clone();
            // Selection order: cost ascending, then path (already sorted
            // stably by index = path order), pinned excluded.
            let mut ranked: Vec<usize> =
                (0..listings.len()).filter(|at| !pinned.contains(at)).collect();
            ranked.sort_by_key(|&at| cost(&listings[at].line));
            Round { name, description, listings, ranked, selected, index: 0 }
        })
        .collect();

    // Budget: namespace lines (with no entries shown) and the pinned
    // listings first, then the round-robin over ranked listings.
    // G-106② — the namespace line's ` // description` suffix counts
    // against the budget exactly like ref catalog.ts:97-111.
    let head = rounds
        .iter()
        .map(|round| {
            cost(&namespace_line(&NamespaceSummary {
                name: round.name.clone(),
                description: round.description.clone(),
                count: round.listings.len(),
                entries: Vec::new(),
            })) + round.selected.iter().map(|&at| cost(&round.listings[at].line)).sum::<usize>()
        })
        .sum::<usize>();
    let mut remaining = INLINE_BUDGET.saturating_sub(head);
    // v2's active-set loop: each round takes one listing per namespace,
    // cheapest first; an exhausted or over-budget namespace drops out.
    let mut active: Vec<usize> = (0..rounds.len()).collect();
    while !active.is_empty() {
        let mut survivors = Vec::new();
        for &at in &active {
            let round = &mut rounds[at];
            let Some(&candidate) = round.ranked.get(round.index) else {
                continue; // exhausted — drops out
            };
            let candidate_cost = cost(&round.listings[candidate].line);
            if candidate_cost > remaining {
                continue; // ranked is cost-ascending: nothing further fits
            }
            round.selected.push(candidate);
            round.index += 1;
            remaining -= candidate_cost;
            if round.index < round.ranked.len() {
                survivors.push(at);
            }
        }
        if survivors.len() == active.len() {
            // Everyone selected this round — a real pass; continue.
            active = survivors;
        } else {
            // At least one namespace dropped; if NOBODY selected
            // anything the loop cannot progress — stop.
            let selected: usize = rounds.iter().map(|round| round.selected.len()).sum();
            if survivors.is_empty() || selected == 0 {
                break;
            }
            active = survivors;
        }
    }

    let namespaces: Vec<NamespaceSummary> = rounds
        .into_iter()
        .map(|round| {
            let mut entries: Vec<Listing> =
                round.selected.iter().map(|&at| round.listings[at].clone()).collect();
            // Selected listings render in path order (catalog.ts:133).
            entries.sort_by(|left, right| left.path.cmp(&right.path));
            NamespaceSummary {
                name: round.name,
                description: round.description,
                count: round.listings.len(),
                entries,
            }
        })
        .collect();
    CatalogSummary {
        total: entries.len(),
        shown: namespaces.iter().map(|namespace| namespace.entries.len()).sum(),
        namespaces,
    }
}

/// `CodeModeInstructions.render`'s prompt (instructions.ts:8-17) — both
/// arms end with the `## Available tools` header.
fn catalog_prompt(has_more: bool) -> String {
    if has_more {
        format!(
            "# Code Mode\n\nUse the `execute` tool to call the tools listed below. They cannot be called directly, and neither can `search`. Both only work inside code you pass to `execute`.\n\nThe catalog is partial. Inside `execute`, use `search(...)` to find a tool, then call it by the `path` in the result. `search` is synchronous. Call it without `await`; it does not return a Promise. Do not guess tool names.\n\n- {}\n\n## Available tools",
            search_signature()
        )
    } else {
        "# Code Mode\n\nUse the `execute` tool to call the tools listed below. They cannot be called directly. They only work inside code you pass to `execute`.\n\nThe catalog is complete. Do not guess tool names.\n\n## Available tools".to_string()
    }
}

/// The rendered instruction text (`CodeModeInstructions.render`).
#[must_use]
pub fn render_instructions(summary: &CatalogSummary) -> String {
    if summary.total == 0 {
        return "No Code Mode tools are currently available. Later Code Mode catalog updates may add or remove tools. Do not call `execute` unless there is at least one available Code Mode tool.".to_string();
    }
    let prompt = catalog_prompt(summary.shown < summary.total);
    let tools: Vec<String> = summary
        .namespaces
        .iter()
        .flat_map(|namespace| {
            std::iter::once(namespace_line(namespace))
                .chain(namespace.entries.iter().map(|entry| entry.line.clone()))
        })
        .collect();
    format!("{prompt}\n\n{}", tools.join("\n"))
}

/// `namespaceLine` (catalog.ts:165-174).
#[must_use]
pub fn namespace_line(namespace: &NamespaceSummary) -> String {
    let count = if namespace.count == 1 {
        "1 tool".to_string()
    } else {
        format!("{} tools", namespace.count)
    };
    let shown = namespace.entries.len();
    let label = if shown == namespace.count {
        count
    } else if shown == 0 {
        format!("{count}, none shown")
    } else {
        format!("{count}, {shown} shown")
    };
    match &namespace.description {
        Some(description) => format!("- {} ({label}) // {description}", namespace.name),
        None => format!("- {} ({label})", namespace.name),
    }
}

/// The summary as the instruction state's JSON value (v2's codec is the
/// `CodeModeCatalog.Summary` schema).
#[must_use]
pub fn summary_value(summary: &CatalogSummary) -> Value {
    json!({
        "total": summary.total,
        "shown": summary.shown,
        "namespaces": summary.namespaces.iter().map(|namespace| {
            // `description` is an optionalKey (G-106②): present only
            // when the namespace registered one.
            let mut value = json!({
                "name": namespace.name,
                "count": namespace.count,
                "entries": namespace.entries.iter().map(|entry| json!({
                    "path": entry.path,
                    "line": entry.line,
                })).collect::<Vec<_>>(),
            });
            if let Some(description) = &namespace.description {
                value["description"] = json!(description);
            }
            value
        }).collect::<Vec<_>>(),
    })
}

fn summary_from_value(value: &Value) -> Option<CatalogSummary> {
    let namespaces = value
        .get("namespaces")?
        .as_array()?
        .iter()
        .map(|namespace| {
            Some(NamespaceSummary {
                name: namespace.get("name")?.as_str()?.to_string(),
                description: namespace
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                count: namespace.get("count")?.as_u64()? as usize,
                entries: namespace
                    .get("entries")?
                    .as_array()?
                    .iter()
                    .map(|entry| {
                        Some(Listing {
                            path: entry.get("path")?.as_str()?.to_string(),
                            line: entry.get("line")?.as_str()?.to_string(),
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(CatalogSummary {
        total: value.get("total")?.as_u64()? as usize,
        shown: value.get("shown")?.as_u64()? as usize,
        namespaces,
    })
}

/// The rendered instruction text for the registry's CURRENT code-mode
/// set. Cached by a digest of every entry's
/// path+description+signature — v2's catalog cache keyed by registry
/// revision + tool-name set (tool.ts:244-248), tightened to also cover
/// in-place description/schema edits (our snapshot carries no revision
/// identity).
#[must_use]
pub fn catalog_instructions(tools: &ToolRegistryState) -> String {
    if tools.executor(EXECUTE_NAME).is_none() {
        // No runtime attached: `execute` is not callable — never
        // advertise it (v2's empty-catalog text).
        return render_instructions(&CatalogSummary::default());
    }
    let entries = code_mode_entries(tools);
    let digest = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        for entry in &entries {
            hasher.update(entry.path.as_bytes());
            hasher.update([0u8]);
            hasher.update(entry.description.as_bytes());
            hasher.update([0u8]);
            hasher.update(entry.signature.as_bytes());
            hasher.update([0u8]);
            hasher.update(entry.search_text.as_bytes());
            hasher.update([0u8]);
            hasher.update([u8::from(entry.pinned)]);
            hasher.update([0u8]);
            if let Some(description) = &entry.namespace_description {
                hasher.update(description.as_bytes());
            }
        }
        format!("{:x}", hasher.finalize())
    };
    static CACHE: Mutex<Option<(String, String)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some((cached_digest, text)) = cache.as_ref() {
        if *cached_digest == digest {
            return text.clone();
        }
    }
    let text = render_instructions(&catalog_summary(&entries));
    *cache = Some((digest, text.clone()));
    text
}

/// The instruction state's `changed` worst case — the whole-text
/// replacement (instructions.ts:33-35); [`instructions_update`] picks
/// the minimal delta when the ref's conditions allow it.
#[must_use]
pub fn instructions_replacement(current: &str) -> String {
    format!("The Code Mode tool catalog has changed. This catalog supersedes the previous Code Mode tool catalog.\n\n{current}")
}

/// `Instructions.diffByKey` (ref instructions/index.ts:207-227).
struct KeyDiff<'a, A> {
    added: Vec<&'a A>,
    removed: Vec<&'a A>,
    changed: Vec<(&'a A, &'a A)>,
}

fn diff_by_key<'a, A>(
    previous: &'a [A],
    current: &'a [A],
    key: impl Fn(&A) -> String,
    changed: impl Fn(&A, &A) -> bool,
) -> KeyDiff<'a, A> {
    use std::collections::{BTreeMap, BTreeSet};
    let current_keys: BTreeSet<String> = current.iter().map(&key).collect();
    let previous_by_key: BTreeMap<String, &A> =
        previous.iter().map(|value| (key(value), value)).collect();
    let added = current.iter().filter(|value| !previous_by_key.contains_key(&key(value))).collect();
    let removed = previous.iter().filter(|value| !current_keys.contains(&key(value))).collect();
    let changed = current
        .iter()
        .filter_map(|value| {
            let before = previous_by_key.get(&key(value))?;
            changed(before, value).then_some((*before, value))
        })
        .collect();
    KeyDiff { added, removed, changed }
}

/// `CodeModeInstructions.update` (ref codemode/instructions.ts:32-124,
/// G-106①): the minimal incremental catalog update — new/changed
/// listing lines, namespace inventory deltas for partial catalogs, a
/// no-longer-available list, and the shorter of delta vs whole-text
/// replacement. `previous`/`current` are the stored summary values;
/// a `previous` that is not a parsable catalog (e.g. the disabled
/// `{"enabled": false}` marker) falls back to whole-text replacement.
pub fn instructions_update(previous: &Value, current: &Value) -> String {
    let replacement = instructions_replacement(&instructions_from_value(current));
    let Some(previous_summary) = summary_from_value(previous) else {
        return replacement;
    };
    let Some(current_summary) = summary_from_value(current) else {
        return replacement;
    };
    catalog_update_text(&previous_summary, &current_summary)
}

/// The typed half of [`instructions_update`] (instructions.ts:32-124).
fn catalog_update_text(previous: &CatalogSummary, current: &CatalogSummary) -> String {
    let replacement = instructions_replacement(&render_instructions(current));
    if current.total == 0 {
        return replacement;
    }
    let previous_complete = previous.shown == previous.total;
    let current_complete = current.shown == current.total;
    if previous_complete != current_complete {
        return replacement;
    }

    // A namespace description appearing, disappearing, or changing
    // regenerates the whole catalog (instructions.ts:41-48).
    let previous_described: Vec<&NamespaceSummary> =
        previous.namespaces.iter().filter(|n| n.description.is_some()).collect();
    let current_described: Vec<&NamespaceSummary> =
        current.namespaces.iter().filter(|n| n.description.is_some()).collect();
    let descriptions = diff_by_key(
        &previous_described,
        &current_described,
        |namespace| namespace.name.clone(),
        |before, after| before.description != after.description,
    );
    if !(descriptions.added.is_empty()
        && descriptions.removed.is_empty()
        && descriptions.changed.is_empty())
    {
        return replacement;
    }

    let previous_entries: Vec<&Listing> =
        previous.namespaces.iter().flat_map(|n| n.entries.iter()).collect();
    let current_entries: Vec<&Listing> =
        current.namespaces.iter().flat_map(|n| n.entries.iter()).collect();
    let diff = diff_by_key(
        &previous_entries,
        &current_entries,
        |entry| entry.path.clone(),
        |before, after| before.line != after.line,
    );
    let entries_changed =
        !(diff.added.is_empty() && diff.removed.is_empty() && diff.changed.is_empty());

    // Length comparisons are UTF-16 code units (JS `.length`).
    let shorter = |delta: String| -> String {
        if delta.encode_utf16().count() < replacement.encode_utf16().count() {
            delta
        } else {
            replacement.clone()
        }
    };

    if !current_complete {
        // Partial catalog: namespace inventory deltas only when the
        // entry set itself did not change (instructions.ts:58-94).
        if entries_changed {
            return replacement;
        }
        let namespaces = diff_by_key(
            &previous.namespaces,
            &current.namespaces,
            |namespace| namespace.name.clone(),
            |before, after| before.count != after.count,
        );
        let changed = !(namespaces.added.is_empty()
            && namespaces.removed.is_empty()
            && namespaces.changed.is_empty());
        if !changed {
            return replacement;
        }
        let mut parts = vec!["The Code Mode tool catalog has changed.".to_string()];
        if !namespaces.added.is_empty() {
            parts.push(format!(
                "New tool namespaces are available: {}.",
                namespaces
                    .added
                    .iter()
                    .map(|namespace| format!("`{}` ({} tools)", namespace.name, namespace.count))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !namespaces.changed.is_empty() {
            parts.push(format!(
                "The following namespace inventories changed; search them again before relying on previous results: {}.",
                namespaces
                    .changed
                    .iter()
                    .map(|(_, after)| format!("`{}` now has {} tools", after.name, after.count))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !namespaces.removed.is_empty() {
            parts.push(format!(
                "The following tool namespaces are no longer available and must not be used: {}.",
                namespaces
                    .removed
                    .iter()
                    .map(|namespace| format!("`{}`", namespace.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        return shorter(parts.join("\n\n"));
    }

    // Complete catalog: entry-level delta (instructions.ts:96-123).
    if !entries_changed {
        return replacement;
    }
    let mut parts = vec!["The Code Mode tool catalog has changed.".to_string()];
    if !diff.added.is_empty() {
        parts.push(
            ["New tools are available in addition to those previously listed:".to_string()]
                .into_iter()
                .chain(diff.added.iter().map(|entry| entry.line.clone()))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if !diff.changed.is_empty() {
        parts.push(
            ["Changed tool listings supersede the previously listed ones:".to_string()]
                .into_iter()
                .chain(diff.changed.iter().map(|(_, after)| after.line.clone()))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if !diff.removed.is_empty() {
        parts.push(format!(
            "The following tools are no longer available and must not be called: {}.",
            diff.removed
                .iter()
                .map(|entry| tool_expression(&entry.path))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    shorter(parts.join("\n\n"))
}

/// The stored value → text (the `initial` render).
#[must_use]
pub fn instructions_from_value(value: &Value) -> String {
    if value.get("enabled") == Some(&Value::Bool(false)) {
        return render_instructions(&CatalogSummary::default());
    }
    match summary_from_value(value) {
        Some(summary) => render_instructions(&summary),
        None => render_instructions(&CatalogSummary::default()),
    }
}

// ---------------------------------------------------------------------------
// Execution limits (codemode.ts:22-41 + interpreter/execute.ts:119-193)
// ---------------------------------------------------------------------------

/// Resource budgets enforced independently during each Code Mode
/// program execution (`ExecutionLimits`, codemode.ts:22-41) — ALL
/// optional, **no defaults: absent = unlimited** (v2's core callsites
/// pass no limits; the mechanism is opt-in at the runtime seam).
///
/// The wire form is camelCase with absent fields omitted, so the
/// unlimited default serializes to `{}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionLimits {
    /// Wall-clock milliseconds before interruption. Result delivery
    /// waits for tool cleanup. No default: absent means no timeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Maximum number of tool calls admitted by the runtime. No
    /// default: absent means unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u64>,
    /// Maximum UTF-8 bytes retained from the result and logs. Warnings
    /// have a separate equal budget; truncation notices and host
    /// formatting are additional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_bytes: Option<u64>,
}

/// `Number.isSafeInteger`'s bound — the ref rejects anything past it.
const MAX_SAFE_INTEGER: u64 = (1u64 << 53) - 1;

/// `validateLimit` (codemode.ts:126-131): a present limit must be a
/// safe integer ≥ `minimum`, else the ref-identical `RangeError` text.
fn validate_limit(name: &str, value: Option<u64>, minimum: u64) -> Result<Option<u64>, String> {
    if let Some(value) = value {
        if value > MAX_SAFE_INTEGER || value < minimum {
            return Err(format!(
                "{name} must be a safe integer greater than or equal to {minimum}."
            ));
        }
    }
    Ok(value)
}

impl ExecutionLimits {
    /// `resolveExecutionLimits` (codemode.ts:133-137) with the ref's
    /// minimums: `timeoutMs ≥ 1`, `maxToolCalls ≥ 0`,
    /// `maxOutputBytes ≥ 0`.
    pub fn resolve(&self) -> Result<Self, String> {
        Ok(Self {
            timeout_ms: validate_limit("timeoutMs", self.timeout_ms, 1)?,
            max_tool_calls: validate_limit("maxToolCalls", self.max_tool_calls, 0)?,
            max_output_bytes: validate_limit("maxOutputBytes", self.max_output_bytes, 0)?,
        })
    }
}

// ---------------------------------------------------------------------------
// G-247 — the module-loading scan (import()/require)
// ---------------------------------------------------------------------------

/// G-247 — the rejection text for a program that tries to load a
/// module. The ref's confined interpreter is structurally incapable of
/// it: its evaluator has no `ImportExpression` arm (interpreter.ts's
/// switch falls through to `unsupportedSyntax`) and `globals.ts`
/// defines no `require`. The guest runs real JavaScript, so the same
/// contract — the verbatim-ref description (tool.ts:64) "Imports,
/// direct filesystem access, and timers are unavailable" — is
/// enforced by statically scanning the program source before it
/// compiles, here at the tool entry AND in the guest (host.js
/// `scanProgramSource`, the identical rule).
pub const MODULE_LOADING_REJECTED: &str =
    "Code Mode programs cannot use import()/require — the sandbox forbids module loading.";

/// Words after which a `/` opens a regex literal (operand position),
/// not a division — the scanner's lexer heuristic.
const REGEX_KEYWORDS: [&str; 14] = [
    "return", "typeof", "instanceof", "in", "of", "new", "delete", "void", "case", "do", "else",
    "yield", "await", "throw",
];

/// Characters after which a `/` opens a regex (operator/punctuation —
/// an operand is expected). Everything else (identifier end, `)`,
/// `]`, `}`, a string/template/regex end) reads as division.
const REGEX_OPENERS: [char; 19] = [
    '(', '[', '{', ',', ';', ':', '=', '!', '?', '&', '|', '+', '-', '*', '%', '^', '~', '<', '>',
];

/// What the scan found in a program source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleLoadToken {
    /// A dynamic `import(...)` call expression.
    Import,
    /// A `require` identifier — call or bare reference.
    Require,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$'
}

fn is_ident_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// G-247 — scan a Code Mode program source for module-loading tokens.
///
/// Token-level approximation: line/block comments, string bodies
/// (`'…'`/`"…"`), template bodies (`` `…` `` with `${…}`
/// interpolations scanned as code — an `import()` hiding inside one
/// is still caught), and regex literals (operator-position heuristic)
/// are stripped before matching; then:
///
/// * a dynamic `import` followed by `(` (whitespace/comments between
///   allowed) rejects — EXCEPT a `.`-preceded property call
///   (`obj.import(…)`: reserved words are legal property names, and a
///   property call never loads a module — this keeps a catalog tool
///   literally named `import` callable);
/// * a `require` identifier rejects in ANY position, `.`-prefixed
///   included (`globalThis.require(…)` is a live module-loading face
///   on require-capable hosts). Pinned false-positive surface: dot
///   access to a property named `require` (`tools.ns.require(…)`);
///   the bracket form `tools.ns["require"](…)` stays legal (the name
///   is a string literal).
///
/// Static `import … from` declarations need no rule: a Function-body
/// cannot carry them (SyntaxError at compile, exactly like the ref
/// where the node does not exist).
pub fn scan_program(code: &str) -> Option<ModuleLoadToken> {
    let chars: Vec<char> = code.chars().collect();
    let len = chars.len();
    let mut i = 0usize;

    // Lexer contexts: `in_template` + the stack of contexts to return
    // to (`Code` = a template closing returns to code; `Tpl` = an
    // interpolation's matching `}` returns to the template).
    #[derive(Clone, Copy, PartialEq)]
    enum Ret {
        Code,
        Tpl,
    }
    let mut in_template = false;
    let mut stack: Vec<Ret> = Vec::new();
    let mut depth = 0usize; // brace depth of the current code region
    let mut prev_code = '\0'; // last significant char of the current code region
    let mut prev_regex_keyword = false; // last word was return/typeof/…

    while i < len {
        let c = chars[i];
        if in_template {
            match c {
                '\\' => i += 2,
                '`' => {
                    in_template = false;
                    stack.pop();
                    prev_code = '`';
                    prev_regex_keyword = false;
                    i += 1;
                }
                '$' if chars.get(i + 1) == Some(&'{') => {
                    stack.push(Ret::Tpl);
                    in_template = false;
                    depth = 0;
                    prev_code = '\0';
                    prev_regex_keyword = false;
                    i += 2;
                }
                _ => i += 1,
            }
            continue;
        }
        // Whitespace and comments are invisible to the token stream.
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < len && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(len);
        } else if c == '"' || c == '\'' {
            i += 1;
            while i < len && chars[i] != c {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            prev_code = '"';
            prev_regex_keyword = false;
        } else if c == '`' {
            stack.push(Ret::Code);
            in_template = true;
            i += 1;
        } else if c == '/'
            && (prev_code == '\0' || prev_regex_keyword || REGEX_OPENERS.contains(&prev_code))
        {
            // Regex literal: skip to the closing `/`, honoring escapes
            // and character classes; a newline means it was never a
            // regex (an unterminated literal is a SyntaxError anyway).
            i += 1;
            let mut in_class = false;
            while i < len {
                let r = chars[i];
                if r == '\\' {
                    i += 2;
                    continue;
                }
                if r == '\n' {
                    break;
                }
                if in_class {
                    if r == ']' {
                        in_class = false;
                    }
                } else if r == '[' {
                    in_class = true;
                } else if r == '/' {
                    break;
                }
                i += 1;
            }
            i += 1;
            while i < len && is_ident_part(chars[i]) {
                i += 1; // flags (gimsuy…)
            }
            prev_code = '_';
            prev_regex_keyword = false;
        } else if is_ident_start(c) {
            let start = i;
            i += 1;
            while i < len && is_ident_part(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match word.as_str() {
                "require" => return Some(ModuleLoadToken::Require),
                "import" if prev_code != '.' => {
                    // Dynamic import iff the next significant token is
                    // `(`. (A `.`-preceded `import` is a property
                    // call — see the doc comment.)
                    let mut j = i;
                    loop {
                        while j < len && chars[j].is_whitespace() {
                            j += 1;
                        }
                        if chars.get(j) == Some(&'/') && chars.get(j + 1) == Some(&'*') {
                            j += 2;
                            while j + 1 < len && !(chars[j] == '*' && chars[j + 1] == '/') {
                                j += 1;
                            }
                            j = (j + 2).min(len);
                            continue;
                        }
                        break;
                    }
                    if chars.get(j) == Some(&'(') {
                        return Some(ModuleLoadToken::Import);
                    }
                }
                _ => {}
            }
            prev_regex_keyword = REGEX_KEYWORDS.contains(&word.as_str());
            prev_code = chars[i - 1];
        } else if c == '{' {
            depth += 1;
            prev_code = c;
            prev_regex_keyword = false;
            i += 1;
        } else if c == '}' {
            if depth > 0 {
                depth -= 1;
            } else if stack.last() == Some(&Ret::Tpl) {
                stack.pop();
                in_template = true;
            }
            prev_code = c;
            prev_regex_keyword = false;
            i += 1;
        } else {
            prev_code = c;
            prev_regex_keyword = false;
            i += 1;
        }
    }
    None
}

/// Deepest nesting a value may have when it crosses to or from the
/// host (`MAX_VALUE_DEPTH`, interpreter/limits.ts:13; the boundary
/// walk in data.ts:53 counts the root at depth 0 and throws entering
/// depth 33 — i.e. 32 container levels are legal, the 33rd is not).
///
/// The ref's sibling structural caps ARE ported, on the guest side of
/// the bridge: `MAX_STRING_LENGTH` (1 << 24) and `MAX_ARRAY_LENGTH`
/// (10_000_000) are checked on every string/array crossing host↔guest
/// (tool inputs, tool results, the return value) with the ref's exact
/// `Invalid string length` / `Invalid array length` RangeErrors, and
/// `MAX_PENDING_PROMISES` (10_000) bounds concurrently pending bridge
/// requests (`Too many pending promises …`). In a real JS engine they
/// bound what one builtin call can build; here they bound what one
/// bridge crossing may carry.
pub const MAX_VALUE_DEPTH: usize = 32;

/// The boundary walk's throw (data.ts:53): `rangeError("Value exceeds
/// the maximum depth of 32.")`.
fn depth_exceeded(value: &Value, depth: usize) -> bool {
    if depth > MAX_VALUE_DEPTH {
        return true;
    }
    match value {
        Value::Object(map) => map.values().any(|item| depth_exceeded(item, depth + 1)),
        Value::Array(items) => items.iter().any(|item| depth_exceeded(item, depth + 1)),
        _ => false,
    }
}

/// Whether `value` nests deeper than [`MAX_VALUE_DEPTH`] — the guard
/// for every value crossing guest↔host (tool inputs, the result).
#[must_use]
pub fn value_depth_exceeded(value: &Value) -> bool {
    depth_exceeded(value, 0)
}

/// The depth guard as a `Result` with the ref's exact message.
pub fn check_value_depth(value: &Value) -> Result<(), String> {
    if value_depth_exceeded(value) {
        Err(format!("Value exceeds the maximum depth of {MAX_VALUE_DEPTH}."))
    } else {
        Ok(())
    }
}

fn utf8_byte_length(value: &str) -> usize {
    value.len()
}

/// `utf8Truncate` (interpreter/execute.ts:122-127): cut at a byte
/// bound and drop a partial UTF-8 sequence (the ref decodes the prefix
/// and strips a trailing replacement character).
fn utf8_truncate(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

/// `boundOutput` (interpreter/execute.ts:129-193): bound the result
/// value, warnings, and logs to `max_output_bytes` UTF-8 bytes.
/// Warnings have a separate equal budget so result data cannot starve
/// diagnostics; logs share what the value left (`max - valueBytes`).
/// Truncation notices (` [result truncated: …]`, `… additional
/// warnings omitted by the output limit.`, `[logs truncated: …]`) are
/// additional, byte-exact against v2. The value arm is skipped when
/// the runtime already reported truncation (the guest pre-bounds its
/// wire value) — the full budget is then spent, as in the ref.
pub fn bound_output(outcome: &mut RunOutcome, max_output_bytes: u64) {
    let max_output_bytes = usize::try_from(max_output_bytes).unwrap_or(usize::MAX);
    let mut truncated = false;

    let mut value_bytes = 0usize;
    if outcome.truncated {
        // The runtime already bounded the value: it consumed the full
        // value budget (the ref sets valueBytes = maxOutputBytes).
        if outcome.value.is_some() {
            value_bytes = max_output_bytes;
        }
    } else if let Some(value) = outcome.value.take() {
        let serialized = serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string());
        let bytes = utf8_byte_length(&serialized);
        if bytes > max_output_bytes {
            truncated = true;
            outcome.value = Some(Value::String(format!(
                "{} [result truncated: {bytes} bytes exceeds the {max_output_bytes}-byte output limit; return a smaller value]",
                utf8_truncate(&serialized, max_output_bytes)
            )));
            value_bytes = max_output_bytes;
        } else {
            outcome.value = Some(value);
            value_bytes = bytes;
        }
    }

    // Warnings: separate equal budget; stop at the first that does not
    // fit (the ref breaks, so an over-budget warning ends the list).
    let total_warnings = outcome.warnings.len();
    let mut kept_warnings = Vec::new();
    let mut warning_bytes = 0usize;
    for warning in outcome.warnings.drain(..) {
        let bytes = serde_json::to_string(&warning)
            .map(|text| utf8_byte_length(&text) + 1)
            .unwrap_or(usize::MAX);
        if warning_bytes.saturating_add(bytes) > max_output_bytes {
            break;
        }
        warning_bytes += bytes;
        kept_warnings.push(warning);
    }
    if kept_warnings.len() < total_warnings {
        truncated = true;
        kept_warnings.push(json!({
            "kind": "Truncated",
            "message": format!(
                "{} additional warnings omitted by the output limit.",
                total_warnings - kept_warnings.len()
            ),
        }));
    }
    outcome.warnings = kept_warnings;

    // Logs: the remaining budget, one byte for each separator.
    let log_budget = max_output_bytes.saturating_sub(value_bytes);
    let total_logs = outcome.logs.len();
    let mut kept_logs = Vec::new();
    let mut log_bytes = 0usize;
    for line in outcome.logs.drain(..) {
        let line_bytes = utf8_byte_length(&line) + 1;
        if log_bytes.saturating_add(line_bytes) > log_budget {
            break;
        }
        log_bytes += line_bytes;
        kept_logs.push(line);
    }
    if kept_logs.len() < total_logs {
        truncated = true;
        kept_logs
            .push(format!("[logs truncated: showing {} of {total_logs} lines]", kept_logs.len()));
    }
    outcome.logs = kept_logs;

    if truncated {
        outcome.truncated = true;
    }
}

/// One tool call observed during an execution (`ExecuteCall`,
/// codemode/tool.ts:24-30) — the progress rows and final metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct CallRecord {
    pub tool: String,
    /// `running` | `completed` | `error`.
    pub status: String,
    /// The model-visible input (v2 `displayInput`: non-empty objects
    /// as-is, other values wrapped, absent omitted).
    pub input: Option<Value>,
}

/// One finished program: the completion value (`None` = `undefined`),
/// the error (a rejected execution), and the observed calls. The
/// `warnings`/`logs`/`truncated` fields mirror the v2 `Result` shape
/// (codemode.ts:94-113) for [`bound_output`]; the guest runtime fills
/// `truncated` when it pre-bounds its wire value.
#[derive(Debug, Clone, Default)]
pub struct RunOutcome {
    pub value: Option<Value>,
    pub error: Option<String>,
    pub tool_calls: Vec<CallRecord>,
    /// Diagnostics that accompany a successful result (v2 `warnings`).
    pub warnings: Vec<Value>,
    /// Captured console output lines (v2 `logs`; the guest swaps
    /// `console.*` into a per-program capture, ref stdlib/console.ts —
    /// G-180①).
    pub logs: Vec<String>,
    /// Set when any output budget truncated the result.
    pub truncated: bool,
}

/// Resolves one nested `tools.<path>(input)` call to its JSON result
/// (`Err` rejects inside the program — the tool failed).
pub type ToolCallFn = Arc<
    dyn Fn(&str, Value) -> futures::future::BoxFuture<'static, Result<Value, String>> + Send + Sync,
>;

/// The Code Mode interpreter seam. v2: the in-process confined
/// interpreter; the Rust port: the plugin host's guest runtime
/// (`codemode.evaluate` over the NDJSON bridge). The implementation
/// owns confinement as it can; [`register_code_mode_with_limits`]'s
/// timeout is the hard bound on the whole execution.
#[async_trait]
pub trait CodeModeRuntime: Send + Sync {
    async fn evaluate(&self, code: &str, catalog: &[CatalogEntry], call: ToolCallFn) -> RunOutcome;

    /// [`Self::evaluate`] under resolved [`ExecutionLimits`] (v2's
    /// opt-in budgets): runtimes that can enforce them (the guest
    /// bridge) override; the default runs unlimited — exactly v2's
    /// core callsites, which pass no limits.
    async fn evaluate_with_limits(
        &self,
        code: &str,
        catalog: &[CatalogEntry],
        limits: &ExecutionLimits,
        call: ToolCallFn,
    ) -> RunOutcome {
        let _ = limits;
        self.evaluate(code, catalog, call).await
    }
}

/// `formatResult` (codemode/tool.ts:310-322): strings verbatim, other
/// values pretty JSON, `undefined` literal; errors carry their message
/// (plus suggestions not already in the message). Successful results
/// append `Warnings:` / `Logs:` sections when present (G-180①: the
/// guest now captures console output into `logs`).
#[must_use]
pub fn format_result(outcome: &RunOutcome) -> String {
    let output = match &outcome.error {
        Some(message) => message.trim().to_string(),
        None => match &outcome.value {
            None => "undefined".to_string(),
            Some(Value::String(text)) => text.clone(),
            Some(other) => {
                serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string())
            }
        },
    };
    // `- [kind] message` per warning (tool.ts:316-318); warnings ride
    // only a successful result.
    let warnings = if outcome.error.is_none() && !outcome.warnings.is_empty() {
        Some(format!(
            "Warnings:\n{}",
            outcome
                .warnings
                .iter()
                .map(|warning| {
                    let kind = warning.get("kind").and_then(Value::as_str).unwrap_or("");
                    let message = warning.get("message").and_then(Value::as_str).unwrap_or("");
                    format!("- [{kind}] {message}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        ))
    } else {
        None
    };
    let logs = (!outcome.logs.is_empty())
        .then(|| format!("Logs:\n{}", outcome.logs.join("\n")));
    [Some(output), warnings, logs]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The `execute` leaf: builds the catalog from the registry at call
/// time, runs the program through the runtime with a HARD timeout,
/// and maps nested calls onto the registered pipeline executors (hooks
/// fire per nested call, exactly v2's `executeTool` wrapping).
///
/// When `limits` are set ([`register_code_mode_with_limits`]) the leaf
/// enforces what lives at the interpreter boundary in v2
/// (interpreter/execute.ts): the `maxToolCalls` admission counter on
/// every nested call, the [`MAX_VALUE_DEPTH`] guard on values crossing
/// guest↔host, the `timeoutMs` program diagnostic, and
/// [`bound_output`] over the finished result. The runtime (guest
/// bridge) enforces the same budgets on its side of the wire.
pub struct CodeModeLeaf {
    pub runtime: Arc<dyn CodeModeRuntime>,
    pub registry: Arc<ToolRegistryState>,
    pub timeout: std::time::Duration,
    /// Opt-in execution budgets (default: unlimited — v2 parity).
    pub limits: ExecutionLimits,
    /// v2 `context.progress({toolCalls})` (codemode/tool.ts:88 — the
    /// `record` helper publishes the running call list). The rust
    /// runtime seam is opaque to per-call updates, so the payload fires
    /// once with the final call list when the program settles.
    pub progress: Option<crate::tool::ProgressFn>,
}

impl CodeModeLeaf {
    fn nested_value(result: &ToolRunResult) -> Value {
        // v2 prefers the declared output, else the joined text, else
        // null (codemode/tool.ts:101-106). TOOL-D1: the widened
        // `ToolRunResult.output` seam is what makes the first arm
        // reachable for output-declaring registrations.
        if let Some(output) = &result.output {
            return output.clone();
        }
        let text = result
            .content
            .iter()
            .filter_map(|part| match part {
                Content::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            Value::Null
        } else {
            Value::String(text)
        }
    }
}

impl super::ToolExecuteWithContext for CodeModeLeaf {
    fn execute<'a>(
        &'a self,
        _services: &'a super::ToolServices,
        input: &'a Value,
        context: &'a ToolCallContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<super::ToolOutcome, super::ToolFailure>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let Some(code) = input.get("code").and_then(Value::as_str) else {
                return Err(super::ToolFailure::message(
                    "Code Mode execution requires a `code` string",
                ));
            };
            // resolveExecutionLimits: invalid budgets reject the call
            // with the ref's RangeError text (v2 throws at make()).
            let limits = match self.limits.resolve() {
                Ok(limits) => limits,
                Err(message) => return Err(super::ToolFailure::message(message)),
            };
            // G-247 — the module-loading scan at the tool entry:
            // `import()`/`require` reject BEFORE the program runs,
            // surfacing as the program's error (exactly the ref's
            // shape, where the confined interpreter fails the program
            // on the unsupported `ImportExpression` node / the missing
            // `require` global — never a tool failure). The guest
            // enforces the identical rule on its side of the wire.
            if scan_program(code).is_some() {
                let mut metadata = BTreeMap::new();
                metadata.insert("toolCalls".to_string(), Value::Array(Vec::new()));
                metadata.insert("error".to_string(), Value::Bool(true));
                return Ok(super::ToolOutcome {
                    content: vec![Content::Text { text: MODULE_LOADING_REJECTED.to_string() }],
                    metadata: Some(metadata),
                    output: None,
                });
            }
            let registry = Arc::clone(&self.registry);
            let call_context = context.clone();
            // ToolRuntime.make's admission counter (tool-runtime.ts:
            // 386-394): exactly `maxToolCalls` calls cross into the
            // pipeline; the (n+1)-th rejects inside the program.
            let admitted = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let max_tool_calls = limits.max_tool_calls;
            let call: ToolCallFn = Arc::new(move |name: &str, input: Value| {
                let registry = Arc::clone(&registry);
                let context = call_context.clone();
                let name = name.to_string();
                let admitted = Arc::clone(&admitted);
                Box::pin(async move {
                    if let Some(max) = max_tool_calls {
                        let nth = admitted.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        if nth > max {
                            return Err(format!(
                                "Execution exceeded its tool-call limit of {max}."
                            ));
                        }
                    }
                    // The boundary walk's depth guard (data.ts:53) on
                    // tool inputs crossing guest→host.
                    check_value_depth(&input)?;
                    // The guest calls by QUALIFIED path (`namespace.name` —
                    // buildToolsTree marks every callable with entry.path);
                    // executors register under the bare name. Direct hit
                    // first (unnamed tools), then the path resolves through
                    // the registration table.
                    let executor = registry.executor(&name).or_else(|| {
                        registry
                            .list()
                            .into_iter()
                            .find(|registration| qualified_path(registration) == name)
                            .and_then(|registration| registry.executor(&registration.name))
                    });
                    let Some(executor) = executor else {
                        return Err(format!("Unknown Code Mode tool: {name}"));
                    };
                    executor
                        .execute_with_context(&input, &context)
                        .await
                        .map(|result| Self::nested_value(&result))
                        .map_err(|failure| failure.message().to_string())
                })
            });
            let entries = code_mode_entries(&self.registry);
            // The limits' timeout wins when it is the tighter bound and
            // produces the ref's program-level TimeoutExceeded
            // diagnostic (interpreter/execute.ts:68); the hard timeout
            // stays the outer belt with its transport-level failure.
            let evaluate = self.runtime.evaluate_with_limits(code, &entries, &limits, call);
            let (outcome, hard) = if let Some(timeout_ms) =
                limits.timeout_ms.filter(|ms| std::time::Duration::from_millis(*ms) < self.timeout)
            {
                let outcome =
                    tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), evaluate)
                        .await;
                match outcome {
                    Ok(outcome) => (outcome, false),
                    Err(_elapsed) => (
                        RunOutcome {
                            error: Some(format!("Execution timed out after {timeout_ms}ms.")),
                            ..RunOutcome::default()
                        },
                        false,
                    ),
                }
            } else {
                let outcome = tokio::time::timeout(self.timeout, evaluate).await;
                match outcome {
                    Ok(outcome) => (outcome, false),
                    Err(_elapsed) => (RunOutcome::default(), true),
                }
            };
            if hard {
                return Err(super::ToolFailure::message(format!(
                    "Code Mode execution timed out after {} ms",
                    self.timeout.as_millis()
                )));
            }
            let mut outcome = outcome;
            // The depth guard on the result crossing guest→host
            // (toBoundary, data.ts:53): a too-deep value is the
            // program's failure, not a transport error.
            if let Some(value) = &outcome.value {
                if let Err(message) = check_value_depth(value) {
                    outcome = RunOutcome {
                        tool_calls: outcome.tool_calls,
                        error: Some(message),
                        ..RunOutcome::default()
                    };
                }
            }
            // boundOutput: budgets apply to the finished result.
            if let Some(max_output_bytes) = limits.max_output_bytes {
                bound_output(&mut outcome, max_output_bytes);
            }
            let output = format_result(&outcome);
            let mut metadata = serde_json::Map::new();
            metadata.insert(
                "toolCalls".to_string(),
                Value::Array(
                    outcome
                        .tool_calls
                        .iter()
                        .map(|record| {
                            let mut object = serde_json::Map::new();
                            object.insert("tool".to_string(), Value::String(record.tool.clone()));
                            object
                                .insert("status".to_string(), Value::String(record.status.clone()));
                            if let Some(input) = &record.input {
                                object.insert("input".to_string(), input.clone());
                            }
                            Value::Object(object)
                        })
                        .collect(),
                ),
            );
            if outcome.error.is_some() {
                metadata.insert("error".to_string(), Value::Bool(true));
            }
            if outcome.truncated {
                metadata.insert("truncated".to_string(), Value::Bool(true));
            }
            // codemode/tool.ts:88 — `context.progress({toolCalls})` with
            // the (settled) call list.
            if let Some(progress) = &self.progress {
                let tool_calls =
                    metadata.get("toolCalls").cloned().unwrap_or(Value::Array(Vec::new()));
                progress(context, serde_json::json!({ "toolCalls": tool_calls }));
            }
            Ok(super::ToolOutcome {
                content: vec![Content::Text { text: output }],
                metadata: Some(metadata.into_iter().collect()),
                output: None,
            })
        })
    }
}

/// Registers the CodeMode surface: the `execute` registration (direct —
/// it IS the CodeMode tool, so the reserved-name check for plugin tools
/// does not apply) plus its pipeline executor (hooks + decode wrap the
/// [`CodeModeLeaf`], tool.ts:237-240's `execute-tool wrapping`).
/// Unlimited variant: v2 core parity (the reference's core callers
/// pass no limits; absent means unlimited).
pub fn register_code_mode(
    state: &Arc<ToolRegistryState>,
    hooks: &Arc<crate::hooks::HookRegistry>,
    services: &Arc<super::ToolServices>,
    runtime: Arc<dyn CodeModeRuntime>,
    timeout: std::time::Duration,
) {
    register_code_mode_with_limits(
        state,
        hooks,
        services,
        runtime,
        timeout,
        ExecutionLimits::default(),
        None,
    )
}

/// [`register_code_mode`] with opt-in [`ExecutionLimits`] (v2
/// `CodeMode.make({ limits })`, codemode.ts:44-53): the leaf enforces
/// the admission counter, the depth guard, the timeout diagnostic, and
/// the output budgets, and passes the limits to the runtime (the guest
/// bridge enforces the same budgets across the wire).
pub fn register_code_mode_with_limits(
    state: &Arc<ToolRegistryState>,
    hooks: &Arc<crate::hooks::HookRegistry>,
    services: &Arc<super::ToolServices>,
    runtime: Arc<dyn CodeModeRuntime>,
    timeout: std::time::Duration,
    limits: ExecutionLimits,
    progress: Option<crate::tool::ProgressFn>,
) {
    state.register(execute_registration());
    let executor = registry::pipeline_executor(
        EXECUTE_NAME,
        execute_input_schema(),
        Arc::new(CodeModeLeaf { runtime, registry: Arc::clone(state), timeout, limits, progress }),
        services,
        hooks,
    );
    state.register_executor(EXECUTE_NAME, executor);
}

// ---------------------------------------------------------------------------
// Tests: byte-pinned signatures + the split + catalog budget
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, description: &str, input: Value) -> CatalogEntry {
        CatalogEntry {
            path: path.to_string(),
            description: description.to_string(),
            signature: signature(path, description, &input),
            search_text: search_text(path, description, &input),
            pinned: false,
            namespace_description: None,
        }
    }

    #[test]
    fn tool_expression_matches_v2() {
        assert_eq!(tool_expression("echo"), "tools.echo");
        assert_eq!(tool_expression("slack.admin-create.x"), "tools.slack[\"admin-create\"].x");
        assert_eq!(tool_expression("2d_get_scene"), "tools[\"2d_get_scene\"]");
    }

    #[test]
    fn signature_renders_v2_pretty_form() {
        let input = json!({
            "type": "object",
            "properties": { "text": { "type": "string", "description": "The text" } },
            "required": ["text"],
        });
        assert_eq!(
            signature("echo", "Echo text", &input),
            "tools.echo({\n  /** The text */\n  text: string,\n}): Promise<void>"
        );
        let empty = json!({ "type": "object", "properties": {} });
        assert_eq!(signature("plain", "No args", &empty), "tools.plain(): Promise<void>");
        let constrained = json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "exclusiveMinimum": 0 },
                "offset": { "type": "integer", "minimum": 0 },
                "mode": { "enum": ["fast", "slow"] },
                "tags": { "type": "array", "items": { "type": "string" } },
                "odd-key": { "type": "boolean" },
                "maybe": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
            },
        });
        // serde_json maps sort alphabetically (no preserve_order): field
        // order is sorted, a documented divergence from v2's insertion
        // order (the search builtin below keeps v2's exact bytes).
        assert_eq!(
            signature("search-ish", "", &constrained),
            "tools[\"search-ish\"]({\n  /** @integer @exclusiveMinimum 0 */\n  limit?: number,\n  maybe?: string | null,\n  mode?: \"fast\" | \"slow\",\n  \"odd-key\"?: boolean,\n  /** @integer @minimum 0 */\n  offset?: number,\n  tags?: Array<string>,\n}): Promise<void>"
        );
    }

    #[test]
    fn search_signature_is_byte_exact() {
        assert_eq!(
            search_signature(),
            "search({\n  query?: string,\n  namespace?: string,\n  /** @integer @exclusiveMinimum 0 */\n  limit?: number,\n  /** @integer @minimum 0 */\n  offset?: number,\n}): {\n  items: Array<{\n      path: string,\n      description: string,\n      signature: string,\n    }>,\n  /** @integer @minimum 0 */\n  remaining: number,\n  next: {\n      /** @integer @minimum 0 */\n      offset: number,\n    } | null,\n}"
        );
    }

    #[test]
    fn split_hides_code_mode_tools_and_appends_execute() {
        let mut greet = ToolRegistration::new("greet", "Greet", json!({}));
        greet.namespace = Some("acme".into());
        let mut direct = ToolRegistration::new("read", "Read", json!({}));
        direct.codemode = false;
        let snapshot = vec![greet, direct.clone()];
        let shaped = shape_request_tools(snapshot.clone(), None, true);
        assert_eq!(shaped.len(), 2, "direct + execute");
        assert_eq!(shaped[0].name, "read");
        assert_eq!(shaped[1].name, "execute");
        assert_eq!(shaped[1].description, EXECUTE_DESCRIPTION);
        assert!(!shaped[1].codemode);
        // Without a registered runtime the snapshot stays flat.
        assert_eq!(shape_request_tools(snapshot, None, false).len(), 2);
        // A wholly-denied `execute` drops the surface entirely.
        let denied: Ruleset = vec![oc_schema::permission::Rule {
            action: "execute".into(),
            resource: "*".into(),
            effect: oc_schema::permission::Effect::Deny,
        }];
        let shaped = shape_request_tools(vec![direct.clone()], Some(&denied), true);
        assert_eq!(shaped.len(), 1);
        assert_eq!(shaped[0].name, "read");
    }

    #[test]
    fn catalog_renders_the_v2_instruction_text() {
        let echo = entry(
            "echo",
            "Echo text\nSecond line here",
            json!({ "type": "object", "properties": { "text": { "type": "string", "description": "The text" } }, "required": ["text"] }),
        );
        let lookup =
            entry("2d_get_scene", "Get the scene", json!({ "type": "object", "properties": {} }));
        let sales = entry(
            "registry.search.sales",
            "Read sales",
            json!({ "type": "object", "properties": {} }),
        );
        let summary = catalog_summary(&[echo.clone(), lookup.clone(), sales.clone()]);
        assert_eq!((summary.total, summary.shown), (3, 3));
        let text = render_instructions(&summary);
        assert_eq!(
            text,
            "# Code Mode\n\nUse the `execute` tool to call the tools listed below. They cannot be called directly. They only work inside code you pass to `execute`.\n\nThe catalog is complete. Do not guess tool names.\n\n## Available tools\n\n- 2d_get_scene (1 tool)\n  - tools[\"2d_get_scene\"](): Promise<void> // Get the scene\n- echo (1 tool)\n  - tools.echo({\n  /** The text */\n  text: string,\n}): Promise<void> // Echo text\n- registry (1 tool)\n  - tools.registry.search.sales(): Promise<void> // Read sales"
        );
        // The empty catalog keeps v2's text.
        assert!(render_instructions(&CatalogSummary::default()).starts_with("No Code Mode tools"));
        // A denied/budgeted catalog advertises `search` (long signatures
        // exhaust the 2000-token inline budget).
        let many: Vec<CatalogEntry> = (0..40)
            .map(|n| {
                let description = format!("Tool number {n}");
                let path = format!("ns{}.tool_{n}", n % 4);
                let mut properties = serde_json::Map::new();
                for field in 0..12 {
                    properties.insert(format!("field_{field}"), json!({ "type": "string" }));
                }
                let input = json!({ "type": "object", "properties": properties });
                entry(&path, &description, input)
            })
            .collect();
        let summary = catalog_summary(&many);
        assert!(
            summary.shown < summary.total,
            "budget engages: {} of {}",
            summary.shown,
            summary.total
        );
        let text = render_instructions(&summary);
        assert!(text.contains("The catalog is partial"), "{text}");
        assert!(text.contains(&format!("- {}", search_signature())));
    }

    #[test]
    fn format_result_matches_v2() {
        assert_eq!(
            format_result(&RunOutcome {
                value: Some(Value::String("hi".into())),
                ..Default::default()
            }),
            "hi"
        );
        assert_eq!(
            format_result(&RunOutcome { value: Some(json!({"a":1})), ..Default::default() }),
            "{\n  \"a\": 1\n}"
        );
        assert_eq!(format_result(&RunOutcome::default()), "undefined");
        assert_eq!(
            format_result(&RunOutcome { value: Some(Value::Null), ..Default::default() }),
            "null"
        );
        assert_eq!(
            format_result(&RunOutcome { error: Some("boom".into()), ..Default::default() }),
            "boom"
        );
    }

    #[test]
    fn summary_value_round_trips() {
        let summary =
            catalog_summary(&[entry("a.b", "B", json!({ "type": "object", "properties": {} }))]);
        let value = summary_value(&summary);
        assert_eq!(summary_from_value(&value), Some(summary));
        assert_eq!(
            instructions_from_value(&value),
            render_instructions(&summary_from_value(&value).unwrap())
        );
    }

    // -- G-106 ------------------------------------------------------------

    /// ③ A multibyte (Chinese) description crossing the truncation
    /// boundary must not panic, and the truncation is by UTF-16 code
    /// units like the ref's JS slice.
    #[test]
    fn g106_multibyte_description_truncation_is_utf16_safe() {
        // 80 CJK chars = 80 UTF-16 units = 240 bytes; the first line
        // exceeds DESCRIPTION_LIMIT (120 units) and cuts mid-multibyte.
        let long: String = "测试".repeat(80);
        let input = json!({ "type": "object", "properties": {} });
        let item = entry("ns.tool", &long, input);
        let line = listing_line(&item);
        // The suffix is ` // ` + exactly 117 UTF-16 units + `...`.
        let suffix = line.rsplit_once(" // ").expect("suffix present").1;
        assert_eq!(suffix.encode_utf16().count(), 117 + 3);
        assert!(suffix.ends_with("..."));
        // A description exactly at a multibyte byte-boundary would have
        // panicked under the old byte slice.
        let boundary: String = "边界".repeat(59); // 118 UTF-16 units, 354 bytes
        let item = entry("ns.tool", &boundary, json!({ "type": "object", "properties": {} }));
        let _ = listing_line(&item);
    }

    /// ④ Cost counts UTF-16 code units (ref catalog.ts:188), so a CJK
    /// listing costs the same as the ref formula, not 3x.
    #[test]
    fn g106_cost_counts_utf16_units() {
        let cjk = "配置工具描述"; // 6 chars, 6 UTF-16 units, 18 bytes
        assert_eq!(cost(cjk), (6 + 2) / 4);
        // 13 ASCII units round to 3, 14 to 4 — Math.round semantics.
        assert_eq!(cost("abcdefghijklm"), 3);
        assert_eq!(cost("abcdefghijklmn"), 4);
    }

    /// ② A registered namespace description renders on the namespace
    /// line, rides the stored value, and counts against the budget.
    #[test]
    fn g106_namespace_description_renders_and_round_trips() {
        let mut with_desc = entry(
            "opencode.models",
            "Search models",
            json!({ "type": "object", "properties": {} }),
        );
        with_desc.namespace_description = Some(
            "Tools for managing OpenCode itself, such as working with sessions, searching the available models, and reading MCP resources.".into(),
        );
        let plain = entry("other.tool", "Other", json!({ "type": "object", "properties": {} }));
        let summary = catalog_summary(&[with_desc, plain]);
        let opencode = summary
            .namespaces
            .iter()
            .find(|namespace| namespace.name == "opencode")
            .expect("opencode namespace");
        assert!(opencode.description.is_some(), "description carried into the summary");
        let text = render_instructions(&summary);
        assert!(
            text.contains("- opencode (1 tool) // Tools for managing OpenCode itself"),
            "namespace line renders the description: {text}"
        );
        assert!(text.contains("- other (1 tool)\n"), "description-less namespace stays bare");
        // The stored value keeps the description (optionalKey).
        let value = summary_value(&summary);
        let reparsed = summary_from_value(&value).expect("round trip");
        assert_eq!(reparsed, summary);
    }

    /// ① The catalog update emits the minimal delta, not the whole
    /// catalog (ref instructions.ts:96-123 — complete catalogs diff by
    /// entry; partial catalogs diff namespace inventories).
    #[test]
    fn g106_catalog_update_emits_minimal_delta() {
        let base = |extra: &[(&str, &str)]| -> CatalogSummary {
            let entries: Vec<CatalogEntry> = [("a.one", "First tool"), ("a.two", "Second tool")]
                .iter()
                .map(|(path, description)| {
                    entry(path, description, json!({ "type": "object", "properties": {} }))
                })
                .chain(extra.iter().map(|(path, description)| {
                    entry(path, description, json!({ "type": "object", "properties": {} }))
                }))
                .collect();
            catalog_summary(&entries)
        };
        let previous = base(&[]);
        let current = base(&[("a.three", "Third tool")]);
        let text = catalog_update_text(&previous, &current);
        assert!(text.starts_with("The Code Mode tool catalog has changed.\n\nNew tools are available in addition to those previously listed:"), "{text}");
        assert!(text.contains("tools.a.three(): Promise<void> // Third tool"), "{text}");
        assert!(!text.contains("# Code Mode"), "no whole-catalog re-render: {text}");
        // Removed tools land in the no-longer-available list.
        let back = catalog_update_text(&current, &previous);
        assert!(
            back.contains(
                "The following tools are no longer available and must not be called: tools.a.three"
            ),
            "{back}"
        );
        // A changed listing line supersedes the previous one.
        let changed = base(&[("a.one", "Edited description")]);
        let text = catalog_update_text(&previous, &changed);
        assert!(
            text.contains("Changed tool listings supersede the previously listed ones:"),
            "{text}"
        );
        assert!(text.contains("tools.a.one(): Promise<void> // Edited description"), "{text}");
        // No change at all still falls back to the replacement (ref
        // returns `replacement` when nothing differs — instructions.ts:96).
        assert!(catalog_update_text(&previous, &previous)
            .starts_with("The Code Mode tool catalog has changed. This catalog supersedes"));
        // A namespace description change regenerates the whole catalog
        // (instructions.ts:41-48).
        let mut with_desc = base(&[]);
        for namespace in &mut with_desc.namespaces {
            namespace.description = Some("desc".into());
        }
        assert!(
            catalog_update_text(&previous, &with_desc)
                .starts_with("The Code Mode tool catalog has changed. This catalog supersedes"),
            "description change forces the whole-text replacement"
        );
    }

    #[test]
    fn pinned_listing_is_always_shown_and_excluded_from_rotation() {
        // catalog.ts:79-90/113-120: a pinned listing rides outside the
        // round-robin — even with a zero remaining budget it stays selected.
        let mut many: Vec<CatalogEntry> = (0..200)
            .map(|index| {
                let path = format!("opencode.tool{index:03}");
                let description = format!("tool number {index} with a description");
                let input = json!({ "type": "object", "properties": {} });
                entry(&path, &description, input)
            })
            .collect();
        let mut session_move = entry(
            "opencode.session_move",
            "Move a session to another directory, or omit sessionID to move the current session.",
            json!({ "type": "object", "properties": {} }),
        );
        session_move.pinned = true;
        many.push(session_move);
        let summary = catalog_summary(&many);
        assert!(summary.shown < summary.total, "budget engages");
        let namespace = summary
            .namespaces
            .iter()
            .find(|namespace| namespace.name == "opencode")
            .expect("opencode namespace");
        assert!(
            namespace.entries.iter().any(|listing| listing.path == "opencode.session_move"),
            "pinned session_move survives the budget: {namespace:?}"
        );
    }
}

/// The executor-level surface (needs the tool-tests harness): a native
/// runtime drives `execute` through the registered pipeline, nesting
/// into pipeline-wrapped tools, surfacing per-call metadata, error
/// text, and the hard timeout — everything but the JS interpreter.
#[cfg(test)]
mod executor_tests {
    use super::super::tests::{call, harness_for_codemode};
    use super::*;
    use crate::location::ToolRunResult;
    use crate::tool::codemode::{
        register_code_mode, shape_request_tools, CallRecord, CatalogEntry, CodeModeRuntime,
        RunOutcome, ToolCallFn, EXECUTE_NAME,
    };
    use async_trait::async_trait;

    /// Interprets the program as whitespace-separated catalog paths —
    /// each resolves through the nested-call fn, recording rows like
    /// the guest does.
    struct ScriptRuntime {
        fail: bool,
    }

    #[async_trait]
    impl CodeModeRuntime for ScriptRuntime {
        async fn evaluate(
            &self,
            code: &str,
            catalog: &[CatalogEntry],
            call: ToolCallFn,
        ) -> RunOutcome {
            let mut outcome = RunOutcome::default();
            for token in code.split_whitespace() {
                let Some(target) = catalog.iter().find(|entry| entry.path == token) else {
                    outcome.error = Some(format!("Unknown Code Mode tool: {token}"));
                    return outcome;
                };
                outcome.tool_calls.push(CallRecord {
                    tool: target.path.clone(),
                    status: "running".into(),
                    input: Some(serde_json::json!({ "name": "wave4" })),
                });
                match call(&target.path, serde_json::json!({ "name": "wave4" })).await {
                    Ok(value) => {
                        outcome.tool_calls.last_mut().expect("row").status = "completed".into();
                        outcome.value = Some(value);
                    }
                    Err(message) => {
                        outcome.tool_calls.last_mut().expect("row").status = "error".into();
                        if self.fail {
                            outcome.error = Some(message);
                            return outcome;
                        }
                        outcome.tool_calls.pop();
                    }
                }
            }
            outcome
        }
    }

    fn codemoded(name: &'static str, text: &'static str, fail: bool) -> crate::tool::BuiltinTool {
        struct Leaf {
            text: &'static str,
            fail: bool,
        }
        impl crate::tool::ToolExecuteWithContext for Leaf {
            fn execute<'a>(
                &'a self,
                _services: &'a crate::tool::ToolServices,
                _input: &'a serde_json::Value,
                _context: &'a crate::location::ToolCallContext,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<crate::tool::ToolOutcome, crate::tool::ToolFailure>,
                        > + Send
                        + 'a,
                >,
            > {
                let text = self.text;
                let fail = self.fail;
                Box::pin(async move {
                    if fail {
                        return Err(crate::tool::ToolFailure::message("kaboom"));
                    }
                    Ok(crate::tool::ToolOutcome {
                        content: vec![oc_schema::tool::Content::Text { text: text.to_string() }],
                        metadata: None,
                        output: None,
                    })
                })
            }
        }
        crate::tool::BuiltinTool {
            name,
            description: format!("Test tool {name}"),
            input: serde_json::json!({ "type": "object", "properties": { "name": { "type": "string" } } }),
            permission: None,
            codemode: true,
            namespace: None,
            execute: Arc::new(Leaf { text, fail }),
        }
    }

    #[tokio::test]
    async fn execute_nests_through_the_registered_pipeline() {
        let harness = harness_for_codemode().await;
        let state = Arc::new(ToolRegistryState::new());
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        crate::tool::registry::register(
            &state,
            &hooks,
            &harness.services,
            codemoded("greet", "hello wave4", false),
        )
        .expect("register greet");
        register_code_mode(
            &state,
            &hooks,
            &harness.services,
            Arc::new(ScriptRuntime { fail: false }),
            std::time::Duration::from_secs(5),
        );
        // The catalog sees greet; the shaped request keeps only execute.
        let entries = crate::tool::codemode::code_mode_entries(&state);
        assert_eq!(entries.iter().map(|entry| entry.path.as_str()).collect::<Vec<_>>(), ["greet"]);
        let shaped = shape_request_tools(state.snapshot(None), None, true);
        assert_eq!(
            shaped.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>(),
            [EXECUTE_NAME]
        );

        let result: ToolRunResult =
            call(&state, EXECUTE_NAME, serde_json::json!({ "code": "greet" }))
                .await
                .expect("execute completes");
        assert_eq!(
            result.content,
            vec![oc_schema::tool::Content::Text { text: "hello wave4".to_string() }]
        );
        let metadata = result.metadata.expect("metadata");
        assert_eq!(
            metadata.get("toolCalls"),
            Some(&serde_json::json!([
                { "tool": "greet", "status": "completed", "input": { "name": "wave4" } }
            ]))
        );
        assert!(!metadata.contains_key("error"));
    }

    #[tokio::test]
    async fn execute_surfaces_nested_errors_and_times_out() {
        let harness = harness_for_codemode().await;
        let state = Arc::new(ToolRegistryState::new());
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        crate::tool::registry::register(
            &state,
            &hooks,
            &harness.services,
            codemoded("boom", "", true),
        )
        .expect("register boom");
        register_code_mode(
            &state,
            &hooks,
            &harness.services,
            Arc::new(ScriptRuntime { fail: true }),
            std::time::Duration::from_secs(5),
        );
        let result = call(&state, EXECUTE_NAME, serde_json::json!({ "code": "boom" }))
            .await
            .expect("a failed program is output, not a tool failure");
        assert_eq!(
            result.content,
            vec![oc_schema::tool::Content::Text { text: "kaboom".to_string() }]
        );
        assert_eq!(result.metadata.expect("metadata").get("error"), Some(&serde_json::json!(true)));

        struct HangingRuntime;
        #[async_trait]
        impl CodeModeRuntime for HangingRuntime {
            async fn evaluate(
                &self,
                _code: &str,
                _catalog: &[CatalogEntry],
                _call: ToolCallFn,
            ) -> RunOutcome {
                futures::future::pending().await
            }
        }
        register_code_mode(
            &state,
            &hooks,
            &harness.services,
            Arc::new(HangingRuntime),
            std::time::Duration::from_millis(50),
        );
        let failure = call(&state, EXECUTE_NAME, serde_json::json!({ "code": "boom" }))
            .await
            .expect_err("hard timeout");
        assert!(failure.message().contains("timed out after 50 ms"), "{}", failure.message());
    }
}

/// Execution-limits parity (v2 `ExecutionLimits`,
/// codemode.ts:22-41 + interpreter/execute.ts + interpreter/limits.ts):
/// validation text, output bounding notices, the tool-call admission
/// cap, the depth guard, and the timeout limit's program-level
/// diagnostic — all byte-exact against the TypeScript reference.
#[cfg(test)]
mod limits_tests {
    use super::super::tests::{call, harness_for_codemode};
    use super::*;
    use serde_json::json;

    /// `{"a":{"a":…}}` with exactly `depth` object levels (the scalar
    /// sits at `depth`, the 33rd level trips the ref's `depth > 32`).
    fn nested(depth: usize) -> Value {
        let mut value = json!({ "a": 1 });
        for _ in 1..depth {
            value = json!({ "a": value });
        }
        value
    }

    // -- validation (validateLimit, codemode.ts:126-137) ---------------------

    #[test]
    fn limits_validation_matches_ref_text() {
        // Absent = unlimited (no defaults — the ref's core callers
        // pass no limits either).
        let resolved = ExecutionLimits::default().resolve().expect("unlimited resolves");
        assert_eq!(resolved.timeout_ms, None);
        assert_eq!(resolved.max_tool_calls, None);
        assert_eq!(resolved.max_output_bytes, None);

        // Below-minimum rejections with the ref-identical RangeError text.
        let bad = ExecutionLimits { timeout_ms: Some(0), ..ExecutionLimits::default() };
        assert_eq!(
            bad.resolve().unwrap_err(),
            "timeoutMs must be a safe integer greater than or equal to 1."
        );
        // The u64 type rules out negatives/non-integers; the remaining
        // rejection for the min-0 fields is the safe-integer ceiling.
        let bad = ExecutionLimits { max_tool_calls: Some(1 << 53), ..ExecutionLimits::default() };
        assert_eq!(
            bad.resolve().unwrap_err(),
            "maxToolCalls must be a safe integer greater than or equal to 0."
        );
        // Zero is VALID for maxToolCalls / maxOutputBytes (min 0) and
        // invalid for timeoutMs (min 1).
        ExecutionLimits {
            max_tool_calls: Some(0),
            max_output_bytes: Some(0),
            ..ExecutionLimits::default()
        }
        .resolve()
        .expect("zero tool/output budgets are valid");

        // Above the safe-integer range (Number.isSafeInteger's bound):
        // same message, ref rejects anything past 2^53-1.
        let bad = ExecutionLimits { max_output_bytes: Some(1 << 53), ..ExecutionLimits::default() };
        assert_eq!(
            bad.resolve().unwrap_err(),
            "maxOutputBytes must be a safe integer greater than or equal to 0."
        );
        // 2^53-1 itself is a safe integer: valid.
        ExecutionLimits { timeout_ms: Some((1 << 53) - 1), ..ExecutionLimits::default() }
            .resolve()
            .expect("2^53-1 is a safe integer");
    }

    // -- boundOutput (interpreter/execute.ts:129-193) ------------------------

    #[test]
    fn bound_output_result_notice_is_byte_exact() {
        let mut outcome = RunOutcome {
            value: Some(json!({ "padding": "x".repeat(100) })),
            ..RunOutcome::default()
        };
        bound_output(&mut outcome, 20);
        assert!(outcome.truncated, "over-budget value marks truncated");
        // serialized = {"padding":"xxx…"} = 114 bytes; 20 retained.
        assert_eq!(
            outcome.value,
            Some(Value::String(
                "{\"padding\":\"xxxxxxxx [result truncated: 114 bytes exceeds the 20-byte output limit; return a smaller value]"
                    .to_string()
            ))
        );
    }

    #[test]
    fn bound_output_truncates_on_utf8_boundaries() {
        // "é" is 2 UTF-8 bytes: serialized = " + 50×2 + " = 102 bytes;
        // 7 retained cuts inside the 4th é — the partial sequence is
        // dropped (ref utf8Truncate's replacement-char strip).
        let mut outcome =
            RunOutcome { value: Some(json!("é".repeat(50))), ..RunOutcome::default() };
        bound_output(&mut outcome, 7);
        assert_eq!(
            outcome.value,
            Some(Value::String(
                "\"ééé [result truncated: 102 bytes exceeds the 7-byte output limit; return a smaller value]"
                    .to_string()
            ))
        );
    }

    #[test]
    fn bound_output_warnings_have_a_separate_equal_budget() {
        let warning = || json!({ "kind": "TimeoutExceeded", "message": "m" });
        let mut outcome = RunOutcome {
            value: Some(Value::Null),
            warnings: vec![warning(), warning(), warning()],
            ..RunOutcome::default()
        };
        // Each warning serializes to 39 bytes (+1) = 40; budget 90
        // keeps two (120 > 90 drops the third).
        bound_output(&mut outcome, 90);
        assert!(outcome.truncated);
        assert_eq!(outcome.warnings.len(), 3);
        assert_eq!(outcome.warnings[0], warning());
        assert_eq!(outcome.warnings[1], warning());
        assert_eq!(
            outcome.warnings[2],
            json!({ "kind": "Truncated", "message": "1 additional warnings omitted by the output limit." })
        );
    }

    #[test]
    fn bound_output_logs_share_the_remaining_budget() {
        let mut outcome = RunOutcome {
            value: Some(Value::Null), // "null" = 4 bytes
            logs: vec!["aaaa".to_string(), "bbbb".to_string()],
            ..RunOutcome::default()
        };
        // Log budget = 60 - 4 = 56? No: budget 10 - 4 = 6 → the first
        // line (5 bytes incl. separator) fits, the second does not.
        bound_output(&mut outcome, 10);
        assert!(outcome.truncated);
        assert_eq!(
            outcome.logs,
            vec!["aaaa".to_string(), "[logs truncated: showing 1 of 2 lines]".to_string()]
        );
    }

    #[test]
    fn bound_output_is_a_noop_under_budget() {
        let value = json!({ "ok": true, "n": 3 });
        let mut outcome = RunOutcome { value: Some(value.clone()), ..RunOutcome::default() };
        bound_output(&mut outcome, 4096);
        assert!(!outcome.truncated);
        assert_eq!(outcome.value, Some(value));
        assert!(outcome.warnings.is_empty());
        assert!(outcome.logs.is_empty());
    }

    #[test]
    fn depth_guard_bounds_nesting_at_32() {
        assert!(!value_depth_exceeded(&nested(32)), "32 levels cross fine");
        assert!(value_depth_exceeded(&nested(33)), "the 33rd level trips the guard");
        assert_eq!(MAX_VALUE_DEPTH, 32);
        // Arrays count as levels too.
        let mut deep = nested(32);
        for _ in 0..2 {
            deep = json!([deep]);
        }
        assert!(value_depth_exceeded(&deep), "arrays nest like objects");
    }

    // -- the executor leaf (register_code_mode_with_limits) ------------------

    /// A runtime that ignores `code` and performs one probe.
    enum Probe {
        /// Return a value nested `n` levels deep.
        ReturnNested(usize),
        /// Call `greet` with an input nested `n` levels deep.
        SendNested(usize),
        /// Call `greet` `n` times.
        Call(usize),
        /// Never settle.
        Hang,
    }

    struct ProbeRuntime(Probe);

    #[async_trait]
    impl CodeModeRuntime for ProbeRuntime {
        async fn evaluate(
            &self,
            code: &str,
            _catalog: &[CatalogEntry],
            call: ToolCallFn,
        ) -> RunOutcome {
            let mut outcome = RunOutcome::default();
            match &self.0 {
                Probe::ReturnNested(depth) => {
                    outcome.value = Some(nested(*depth));
                }
                Probe::SendNested(depth) => match call("greet", nested(*depth)).await {
                    Ok(value) => outcome.value = Some(value),
                    Err(message) => outcome.error = Some(message),
                },
                Probe::Call(times) => {
                    for _ in 0..*times {
                        outcome.tool_calls.push(CallRecord {
                            tool: "greet".to_string(),
                            status: "running".to_string(),
                            input: Some(json!({ "name": "limits" })),
                        });
                        match call("greet", json!({ "name": "limits" })).await {
                            Ok(value) => {
                                outcome.tool_calls.last_mut().expect("row").status =
                                    "completed".into();
                                outcome.value = Some(value);
                            }
                            Err(message) => {
                                if message.starts_with("Execution exceeded its tool-call limit") {
                                    // The guest throws before recording a rejected call.
                                    outcome.tool_calls.pop();
                                } else {
                                    outcome.tool_calls.last_mut().expect("row").status =
                                        "error".into();
                                }
                                outcome.error = Some(message);
                                break;
                            }
                        }
                    }
                }
                Probe::Hang => {
                    let _ = code;
                    futures::future::pending::<()>().await;
                }
            }
            outcome
        }
    }

    async fn limited_harness(
        runtime: Arc<dyn CodeModeRuntime>,
        limits: ExecutionLimits,
    ) -> (Arc<ToolRegistryState>, Arc<crate::hooks::HookRegistry>) {
        limited_harness_with_progress(runtime, limits, None).await
    }

    async fn limited_harness_with_progress(
        runtime: Arc<dyn CodeModeRuntime>,
        limits: ExecutionLimits,
        progress: Option<crate::tool::ProgressFn>,
    ) -> (Arc<ToolRegistryState>, Arc<crate::hooks::HookRegistry>) {
        let harness = harness_for_codemode().await;
        let state = Arc::new(ToolRegistryState::new());
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        crate::tool::registry::register(
            &state,
            &hooks,
            &harness.services,
            crate::tool::BuiltinTool {
                name: "greet",
                description: "Greet".to_string(),
                input: json!({ "type": "object", "properties": {} }),
                permission: None,
                codemode: true,
                namespace: None,
                execute: {
                    struct OkLeaf;
                    impl crate::tool::ToolExecuteWithContext for OkLeaf {
                        fn execute<'a>(
                            &'a self,
                            _services: &'a crate::tool::ToolServices,
                            _input: &'a serde_json::Value,
                            _context: &'a crate::location::ToolCallContext,
                        ) -> std::pin::Pin<
                            Box<
                                dyn std::future::Future<
                                        Output = Result<
                                            crate::tool::ToolOutcome,
                                            crate::tool::ToolFailure,
                                        >,
                                    > + Send
                                    + 'a,
                            >,
                        > {
                            Box::pin(async move {
                                Ok(crate::tool::ToolOutcome {
                                    content: vec![oc_schema::tool::Content::Text {
                                        text: "hi".to_string(),
                                    }],
                                    metadata: None,
                                    output: None,
                                })
                            })
                        }
                    }
                    Arc::new(OkLeaf)
                },
            },
        )
        .expect("register greet");
        register_code_mode_with_limits(
            &state,
            &hooks,
            &harness.services,
            runtime,
            std::time::Duration::from_secs(5),
            limits,
            progress,
        );
        (state, hooks)
    }

    fn text_of(result: &crate::location::ToolRunResult) -> String {
        match &result.content[0] {
            oc_schema::tool::Content::Text { text } => text.clone(),
            _ => String::new(),
        }
    }

    /// G-74: the codemode leaf emits `context.progress({toolCalls})`
    /// (codemode/tool.ts:88) — the payload observed on the channel.
    #[tokio::test]
    async fn tool_codemode_progress_payload_carries_tool_calls() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let (state, _hooks) = limited_harness_with_progress(
            Arc::new(ProbeRuntime(Probe::Call(2))),
            ExecutionLimits::default(),
            Some(Arc::new(move |_context, payload| {
                sink.lock().expect("progress sink").push(payload);
            })),
        )
        .await;
        let result =
            call(&state, EXECUTE_NAME, json!({ "code": "two" })).await.expect("program runs");
        assert!(result.metadata.is_some());
        let seen = seen.lock().expect("progress sink");
        assert_eq!(seen.len(), 1, "one progress emission");
        let calls = seen[0].get("toolCalls").and_then(Value::as_array).expect("toolCalls payload");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["tool"], "greet");
        assert_eq!(calls[0]["status"], "completed");
    }

    #[tokio::test]
    async fn tool_call_cap_rejects_beyond_the_limit() {
        let (state, _hooks) = limited_harness(
            Arc::new(ProbeRuntime(Probe::Call(3))),
            ExecutionLimits { max_tool_calls: Some(2), ..ExecutionLimits::default() },
        )
        .await;
        let result = call(&state, EXECUTE_NAME, json!({ "code": "three" }))
            .await
            .expect("a capped program is output, not a tool failure");
        assert_eq!(text_of(&result), "Execution exceeded its tool-call limit of 2.");
        let metadata = result.metadata.expect("metadata");
        assert_eq!(metadata.get("error"), Some(&json!(true)));
        // Exactly the limit was admitted.
        let calls = metadata.get("toolCalls").and_then(|v| v.as_array()).expect("toolCalls");
        assert_eq!(calls.len(), 2);
    }

    #[tokio::test]
    async fn depth_guard_rejects_deep_inputs_and_values() {
        // Tool inputs crossing guest→host are rejected at the boundary.
        let (state, _hooks) = limited_harness(
            Arc::new(ProbeRuntime(Probe::SendNested(33))),
            ExecutionLimits::default(),
        )
        .await;
        let result = call(&state, EXECUTE_NAME, json!({ "code": "deep input" }))
            .await
            .expect("a rejected call surfaces as the program's error");
        assert_eq!(text_of(&result), "Value exceeds the maximum depth of 32.");

        // The returned value crossing guest→host is guarded too.
        let (state, _hooks) = limited_harness(
            Arc::new(ProbeRuntime(Probe::ReturnNested(33))),
            ExecutionLimits::default(),
        )
        .await;
        let result = call(&state, EXECUTE_NAME, json!({ "code": "deep value" }))
            .await
            .expect("a deep value surfaces as the program's error");
        assert_eq!(text_of(&result), "Value exceeds the maximum depth of 32.");

        // 32 levels cross fine.
        let (state, _hooks) = limited_harness(
            Arc::new(ProbeRuntime(Probe::ReturnNested(32))),
            ExecutionLimits::default(),
        )
        .await;
        let result = call(&state, EXECUTE_NAME, json!({ "code": "ok value" }))
            .await
            .expect("32 levels are within the guard");
        assert_ne!(text_of(&result), "Value exceeds the maximum depth of 32.");
    }

    #[tokio::test]
    async fn timeout_limit_yields_the_ref_diagnostic() {
        let (state, _hooks) = limited_harness(
            Arc::new(ProbeRuntime(Probe::Hang)),
            ExecutionLimits { timeout_ms: Some(60), ..ExecutionLimits::default() },
        )
        .await;
        let result = call(&state, EXECUTE_NAME, json!({ "code": "hang" }))
            .await
            .expect("the timeout is the program's error, not a tool failure");
        assert_eq!(text_of(&result), "Execution timed out after 60ms.");
        assert_eq!(result.metadata.expect("metadata").get("error"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn invalid_limits_fail_the_execute_call() {
        let (state, _hooks) = limited_harness(
            Arc::new(ProbeRuntime(Probe::Call(1))),
            ExecutionLimits { timeout_ms: Some(0), ..ExecutionLimits::default() },
        )
        .await;
        let failure = call(&state, EXECUTE_NAME, json!({ "code": "any" }))
            .await
            .expect_err("invalid limits reject the call");
        assert_eq!(
            failure.message(),
            "timeoutMs must be a safe integer greater than or equal to 1."
        );
    }

    #[tokio::test]
    async fn output_limit_truncates_the_leaf_result() {
        // The native-runtime path: the leaf bounds a value the runtime
        // returned un-bounded (the guest path pre-bounds on its side
        // and flags `truncated`).
        let (state, _hooks) = limited_harness(
            Arc::new(ProbeRuntime(Probe::ReturnNested(1))), // {"a":1} = 7 bytes
            ExecutionLimits { max_output_bytes: Some(4), ..ExecutionLimits::default() },
        )
        .await;
        let result = call(&state, EXECUTE_NAME, json!({ "code": "wide" }))
            .await
            .expect("a truncated program still succeeds");
        assert_eq!(
            text_of(&result),
            "{\"a\" [result truncated: 7 bytes exceeds the 4-byte output limit; return a smaller value]"
        );
        assert_eq!(result.metadata.expect("metadata").get("truncated"), Some(&json!(true)));
    }

    /// G-180① — `formatResult` (tool.ts:310-322): a successful result
    /// appends `Warnings:`/`Logs:` sections when present, joined with a
    /// blank line; sections drop when empty.
    #[test]
    fn format_result_appends_warning_and_log_sections() {
        let base = RunOutcome { value: Some(json!("done")), ..RunOutcome::default() };
        assert_eq!(format_result(&base), "done");

        let with_logs = RunOutcome {
            value: Some(json!("done")),
            logs: vec!["hello 42".to_string(), "[warn] careful".to_string()],
            ..RunOutcome::default()
        };
        assert_eq!(format_result(&with_logs), "done\n\nLogs:\nhello 42\n[warn] careful");

        let with_both = RunOutcome {
            value: Some(json!(1)),
            warnings: vec![json!({ "kind": "Truncated", "message": "cut short" })],
            logs: vec!["one".to_string()],
            ..RunOutcome::default()
        };
        assert_eq!(
            format_result(&with_both),
            "1\n\nWarnings:\n- [Truncated] cut short\n\nLogs:\none"
        );

        // Warnings ride only a successful result (Failure has none).
        let failed = RunOutcome {
            error: Some("boom".to_string()),
            warnings: vec![json!({ "kind": "Truncated", "message": "cut" })],
            logs: vec!["before".to_string()],
            ..RunOutcome::default()
        };
        assert_eq!(format_result(&failed), "boom\n\nLogs:\nbefore");
    }
}

/// G-247 — the module-loading scan (`import()`/`require`): token-level
/// acceptance/rejection corpus (comments, strings, template bodies,
/// regex literals stripped; `${…}` interpolations scanned as code),
/// plus the executor leaf's rejection surface (the program fails with
/// the scan's error text and the runtime never runs).
#[cfg(test)]
mod g247_tests {
    use super::super::tests::{call, harness_for_codemode};
    use super::*;
    use serde_json::json;

    #[test]
    fn scan_rejects_module_loading() {
        let rejected: &[(&str, ModuleLoadToken)] = &[
            // Dynamic import — the direct escape the gap entry cites.
            (r#"await import("node:fs")"#, ModuleLoadToken::Import),
            (r#"return import("node:child_process").then(m => m.spawn("id"))"#, ModuleLoadToken::Import),
            // Whitespace/newline variants between `import` and `(`.
            (r#"import ("node:fs")"#, ModuleLoadToken::Import),
            ("import\n(\n'node:fs'\n)", ModuleLoadToken::Import),
            // A block comment between `import` and `(` is still a call.
            (r#"import/*g247*/("node:fs")"#, ModuleLoadToken::Import),
            // Template specifier — the call shape is what flags.
            ("return import(`node:fs`);", ModuleLoadToken::Import),
            // Hiding inside a template interpolation.
            (r#"return `fs: ${import("node:fs")}`;"#, ModuleLoadToken::Import),
            // …and inside a NESTED interpolation.
            (r#"return `a ${ `b ${ require("fs") } c` } d`;"#, ModuleLoadToken::Require),
            // require — call, bare reference, and property positions
            // (globalThis.require is a live module-loading face on
            // require-capable hosts).
            (r#"const fs = require("node:fs")"#, ModuleLoadToken::Require),
            ("const r = require;", ModuleLoadToken::Require),
            (r#"return globalThis.require("fs")"#, ModuleLoadToken::Require),
            ("x?.require", ModuleLoadToken::Require),
            // Inside a block — position does not matter.
            (r#"if (x) { await import("node:fs"); }"#, ModuleLoadToken::Import),
        ];
        for (code, expected) in rejected {
            assert_eq!(
                scan_program(code),
                Some(*expected),
                "must reject: {code}"
            );
        }
    }

    #[test]
    fn scan_accepts_legal_programs() {
        let accepted: &[&str] = &[
            // Plain programs.
            "return 1 + 1;",
            r#"const r = await tools.greet({ name: "x" }); return r;"#,
            // The words inside STRING bodies never flag.
            r#"return "require is just a word: require(\"fs\")";"#,
            r#"return 'import("node:fs") was rejected';"#,
            // …inside COMMENTS.
            "// import(\"node:fs\") would be rejected — but not here\nreturn 7;",
            "/* require(\"fs\") */ return 8;",
            // Template bodies (no interpolation) are string content.
            "return `require(\"fs\") untouchable`;",
            // Word boundaries: longer identifiers containing the token.
            "const importer = 1, my_require = 2, required = 3; return importer + my_require + required;",
            // Property calls never load modules: reserved words are
            // legal property names (`obj.import(…)`).
            "const box = { import: (x) => x }; return box.import(41);",
            // The bracket form keeps a `require`-named path callable
            // (the name is a string literal).
            r#"return tools["require"]({ x: 1 });"#,
            // Regex literals are skipped (operator-position heuristic).
            r#"const re = /require\(/; return re.test("require(");"#,
            r#"const f = () => /import\(/.test("x"); return f;"#,
            // Division after operands reads as division, not regex.
            "const ratio = width / 2 / scale; return ratio;",
            // A static import DECLARATION carries no call shape (and a
            // Function body cannot hold one — SyntaxError at compile,
            // exactly like the ref's missing node).
            r#"import fs from "node:fs";"#,
            "import.meta.url",
            // Escapes inside strings keep the lexer anchored.
            r#"const s = "escaped \" quote and ' solo"; return s.length;"#,
            "const t = `back \\` tick`; return t;",
            // A regex character class containing a slash.
            r#"const re = /a[/]b/; return re.source;"#,
        ];
        for code in accepted {
            assert_eq!(scan_program(code), None, "must accept: {code}");
        }
    }

    #[test]
    fn scan_rejection_text_is_pinned() {
        assert_eq!(
            MODULE_LOADING_REJECTED,
            "Code Mode programs cannot use import()/require — the sandbox forbids module loading."
        );
    }

    /// The executor leaf rejects before the runtime: the scan's error
    /// is the program's OUTPUT (error metadata), never a tool failure,
    /// and the runtime is never invoked.
    #[tokio::test]
    async fn g247_leaf_rejects_module_loading_before_the_runtime() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let harness = harness_for_codemode().await;
        let state = Arc::new(ToolRegistryState::new());
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        let invoked = Arc::new(AtomicBool::new(false));
        struct ProbeRuntime(Arc<AtomicBool>);
        #[async_trait]
        impl CodeModeRuntime for ProbeRuntime {
            async fn evaluate(
                &self,
                _code: &str,
                _catalog: &[CatalogEntry],
                _call: ToolCallFn,
            ) -> RunOutcome {
                self.0.store(true, Ordering::SeqCst);
                RunOutcome { value: Some(json!("runtime ran")), ..RunOutcome::default() }
            }
        }
        register_code_mode(
            &state,
            &hooks,
            &harness.services,
            Arc::new(ProbeRuntime(Arc::clone(&invoked))),
            std::time::Duration::from_secs(5),
        );

        for code in [
            r#"await import("node:fs")"#,
            r#"return require("fs").readFileSync("/etc/hostname", "utf8");"#,
            "return globalThis.require(\"child_process\");",
        ] {
            let result = call(&state, EXECUTE_NAME, json!({ "code": code }))
                .await
                .expect("the rejection is the program's output, not a tool failure");
            let text = match &result.content[0] {
                Content::Text { text } => text.clone(),
                Content::File { .. } => String::new(),
            };
            assert_eq!(text, MODULE_LOADING_REJECTED, "code: {code}");
            let metadata = result.metadata.expect("metadata");
            assert_eq!(metadata.get("error"), Some(&json!(true)));
            assert_eq!(metadata.get("toolCalls"), Some(&json!([])));
        }
        assert!(!invoked.load(Ordering::SeqCst), "the runtime must not run for rejected programs");

        // A legal program mentioning `require` inside a string still
        // reaches the runtime.
        let result = call(
            &state,
            EXECUTE_NAME,
            json!({ "code": r#"return "the word require stays legal in strings";"# }),
        )
        .await
        .expect("legal program runs");
        let text = match &result.content[0] {
            Content::Text { text } => text.clone(),
            Content::File { .. } => String::new(),
        };
        assert_eq!(text, "runtime ran");
        assert!(invoked.load(Ordering::SeqCst));
    }
}
