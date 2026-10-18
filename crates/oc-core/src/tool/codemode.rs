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
//! * No `search`-over-inventory on the Rust side of the catalog — the
//!   guest exposes the v2-ranked `search` builtin over the same entries
//!   the catalog rendered (instructions stay faithful).
//! * Nested results pass the joined text of the tool's content parts
//!   (v2 prefers the declared output schema value; our registrations
//!   carry none — the common single-text-part result is identical).
//! * The confined-interpreter guarantees (no imports/timers, loop
//!   budgets, last-expression completion values) are the guest
//!   sandbox's hard timeout + the catalog allowlist instead; see the
//!   oc-plugin-host `codemode.evaluate` handler notes.

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

/// The code-mode half of a registry snapshot (permission filtering is
/// the caller's; the catalog mirrors the request's tool set).
#[must_use]
pub fn code_mode_entries(tools: &ToolRegistryState) -> Vec<CatalogEntry> {
    tools
        .snapshot(None)
        .iter()
        .filter(|tool| tool.codemode)
        .map(|tool| CatalogEntry {
            path: qualified_path(tool),
            description: tool.description.clone(),
            signature: signature(&qualified_path(tool), &tool.description, &tool.input),
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
    text.len().div_ceil(CHARACTERS_PER_TOKEN)
}

/// The listing suffix: `  - {signature} // {first description line}`.
fn listing_line(entry: &CatalogEntry) -> String {
    let first_line = entry.description.split('\n').next().unwrap_or_default().trim().to_string();
    let description = if first_line.len() > DESCRIPTION_LIMIT {
        format!("{}...", &first_line[..DESCRIPTION_LIMIT - 3])
    } else {
        first_line
    };
    let suffix = if description.is_empty() { String::new() } else { format!(" // {description}") };
    format!("  - {}{}", entry.signature, suffix)
}

/// `CodeModeCatalog.summarize` (catalog.ts:54-140): group by the first
/// path segment (sorted), then select full listings shortest-first, one
/// per namespace per round, until the inline budget is spent. Pinned
/// listings don't exist here (registrations carry no `pinned` option),
/// so every listing rides the round-robin.
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
        listings: Vec<Listing>,
        ranked: Vec<usize>, // listing indexes, cost-ascending then path
        selected: Vec<usize>,
        index: usize,
    }
    let mut rounds: Vec<Round> = groups
        .into_iter()
        .map(|(name, group)| {
            // Listings sorted by path inside the namespace.
            let mut listings: Vec<Listing> = group
                .iter()
                .map(|entry| Listing { path: entry.path.clone(), line: listing_line(entry) })
                .collect();
            listings.sort_by(|left, right| left.path.cmp(&right.path));
            // Selection order: cost ascending, then path (already sorted
            // stably by index = path order).
            let mut ranked: Vec<usize> = (0..listings.len()).collect();
            ranked.sort_by_key(|&at| cost(&listings[at].line));
            Round { name, listings, ranked, selected: Vec::new(), index: 0 }
        })
        .collect();

    // Budget: namespace lines (with no entries shown) first, then the
    // round-robin over ranked listings.
    let head = rounds
        .iter()
        .map(|round| {
            cost(&namespace_line(&NamespaceSummary {
                name: round.name.clone(),
                description: None,
                count: round.listings.len(),
                entries: Vec::new(),
            }))
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
                description: None,
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
        "namespaces": summary.namespaces.iter().map(|namespace| json!({
            "name": namespace.name,
            "count": namespace.count,
            "entries": namespace.entries.iter().map(|entry| json!({
                "path": entry.path,
                "line": entry.line,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
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

/// The instruction state's `changed` render — v2's full-replacement
/// fallback (`CodeModeInstructions.update` worst case; the minimal
/// delta forms are a Phase 10 refinement).
#[must_use]
pub fn instructions_replacement(current: &str) -> String {
    format!("The Code Mode tool catalog has changed. This catalog supersedes the previous Code Mode tool catalog.\n\n{current}")
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
// The runtime seam + the executor leaf
// ---------------------------------------------------------------------------

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
/// the error (a rejected execution), and the observed calls.
#[derive(Debug, Clone, Default)]
pub struct RunOutcome {
    pub value: Option<Value>,
    pub error: Option<String>,
    pub tool_calls: Vec<CallRecord>,
}

/// Resolves one nested `tools.<path>(input)` call to its JSON result
/// (`Err` rejects inside the program — the tool failed).
pub type ToolCallFn = Arc<
    dyn Fn(&str, Value) -> futures::future::BoxFuture<'static, Result<Value, String>> + Send + Sync,
>;

/// The Code Mode interpreter seam. v2: the in-process confined
/// interpreter; the Rust port: the plugin host's guest runtime
/// (`codemode.evaluate` over the NDJSON bridge). The implementation
/// owns confinement as it can; [`register_code_mode`]'s timeout is the
/// hard bound on the whole execution.
#[async_trait]
pub trait CodeModeRuntime: Send + Sync {
    async fn evaluate(&self, code: &str, catalog: &[CatalogEntry], call: ToolCallFn) -> RunOutcome;
}

/// `formatResult` (codemode/tool.ts:310-322): strings verbatim, other
/// values pretty JSON, `undefined` literal; errors carry their message.
#[must_use]
pub fn format_result(outcome: &RunOutcome) -> String {
    match &outcome.error {
        Some(message) => message.trim().to_string(),
        None => match &outcome.value {
            None => "undefined".to_string(),
            Some(Value::String(text)) => text.clone(),
            Some(other) => {
                serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string())
            }
        },
    }
}

/// The `execute` leaf: builds the catalog from the registry at call
/// time, runs the program through the runtime with a HARD timeout, and
/// maps nested calls onto the registered pipeline executors (hooks fire
/// per nested call, exactly v2's `executeTool` wrapping).
pub struct CodeModeLeaf {
    pub runtime: Arc<dyn CodeModeRuntime>,
    pub registry: Arc<ToolRegistryState>,
    pub timeout: std::time::Duration,
}

impl CodeModeLeaf {
    fn nested_value(result: &ToolRunResult) -> Value {
        // v2 prefers the declared output, else the joined text, else
        // null. Registrations carry no output schema — the text join.
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
            let registry = Arc::clone(&self.registry);
            let call_context = context.clone();
            let call: ToolCallFn = Arc::new(move |name: &str, input: Value| {
                let registry = Arc::clone(&registry);
                let context = call_context.clone();
                let name = name.to_string();
                Box::pin(async move {
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
            let outcome =
                tokio::time::timeout(self.timeout, self.runtime.evaluate(code, &entries, call))
                    .await;
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(_elapsed) => {
                    return Err(super::ToolFailure::message(format!(
                        "Code Mode execution timed out after {} ms",
                        self.timeout.as_millis()
                    )));
                }
            };
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
pub fn register_code_mode(
    state: &Arc<ToolRegistryState>,
    hooks: &Arc<crate::hooks::HookRegistry>,
    services: &Arc<super::ToolServices>,
    runtime: Arc<dyn CodeModeRuntime>,
    timeout: std::time::Duration,
) {
    state.register(execute_registration());
    let executor = registry::pipeline_executor(
        EXECUTE_NAME,
        execute_input_schema(),
        Arc::new(CodeModeLeaf { runtime, registry: Arc::clone(state), timeout }),
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
