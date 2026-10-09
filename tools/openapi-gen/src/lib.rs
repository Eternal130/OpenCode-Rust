//! openapi.json -> Rust route table + client types/methods generator.
//!
//! Parses the pinned OpenAPI document (`tools/openapi-gen/openapi.json`,
//! copied verbatim from `packages/protocol/openapi.json` in opencode v2.0.24)
//! and emits three deterministic artifacts:
//!
//! * `crates/oc-server/src/routes.gen.rs` — static `ROUTES` table.
//! * `crates/oc-client/src/types.gen.rs` — wire-faithful serde types.
//! * `crates/oc-client/src/methods.gen.rs` — namespaced client methods.
//!
//! Drift control: the pinned spec's sha256 is embedded in every artifact and
//! the `gen.rs` integration test asserts the checked-in files match
//! regeneration byte-for-byte.
//
// The emitter bodies intentionally build strings via `push_str(&format!..)`.
#![allow(
    clippy::too_many_lines,
    clippy::format_push_string,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::match_same_arms,
    clippy::doc_markdown,
    clippy::redundant_closure,
    clippy::map_unwrap_or
)]

pub mod emit_client;
pub mod emit_routes;
pub mod groups;
pub mod model;
pub mod naming;
pub mod tygen;

use serde_json::Value;

pub struct Generated {
    pub routes_rs: String,
    pub types_rs: String,
    pub methods_rs: String,
    pub warnings: Vec<String>,
    pub operation_count: usize,
    pub spec_sha256: String,
}

/// The pinned spec, compiled into the binary (used by tests and `--check`).
pub const PINNED_SPEC: &str = include_str!("../openapi.json");

pub fn pinned_sha256() -> String {
    oc_util::hash::sha256_hex(PINNED_SPEC)
}

/// Path of the pinned spec on disk (runtime override target).
pub fn spec_path() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/openapi.json").to_owned()
}

pub fn generate_from_spec(spec_json: &str) -> Generated {
    let spec: Value =
        serde_json::from_str(spec_json).expect("pinned openapi.json must be valid JSON");
    let sha = oc_util::hash::sha256_hex(spec_json);
    let operations = model::parse_operations(&spec);
    assert!(
        operations.len() == 141,
        "expected 141 operations from the pinned spec, found {} — \
         the spec drifted; re-pin deliberately and update this check",
        operations.len()
    );
    let schemas = model::schemas(&spec);
    let mut gen = tygen::TyGen::new(&schemas);
    for name in schemas.keys() {
        gen.named_type(name);
    }
    let mut infos: Vec<emit_client::OpInfo> = Vec::with_capacity(operations.len());
    for op in &operations {
        let types = gen.op_types(op);
        let (module, method) = groups::classify(&op.operation_id);
        infos.push(emit_client::OpInfo { op: op.clone(), types, module, method });
    }
    let warnings = std::mem::take(&mut gen.warnings);
    let def_count = gen.defs().len();

    let mut types_rs = String::new();
    types_rs.push_str(emit_routes::ROUTES_HEADER);
    types_rs.push_str(&format!("// Source: tools/openapi-gen/openapi.json (sha256 {sha})\n"));
    types_rs.push_str(&format!(
        "//! Wire types for the opencode HttpApi ({def_count} generated types).\n"
    ));
    types_rs
        .push_str("//! Field names use exact wire spellings via `#[serde(rename)]` — camelCase,\n");
    types_rs.push_str("//! including `ID` suffixes (`sessionID`, `parentID`, ...).\n");
    types_rs
        .push_str("//! Unions: discriminated where the schema provides a singleton-enum tag,\n");
    types_rs.push_str("//! `#[serde(untagged)]` otherwise (variant order follows the spec).\n");
    types_rs.push_str("#![allow(clippy::large_enum_variant, clippy::struct_excessive_bools)]\n");
    types_rs.push_str("#![allow(clippy::doc_markdown, clippy::must_use_candidate)]\n\n");
    for def in gen.defs() {
        types_rs.push_str(&def.code);
        types_rs.push('\n');
    }

    let routes_rs = emit_routes::emit(&infos, &sha);
    let methods_rs = emit_client::emit(&infos);

    Generated {
        routes_rs,
        types_rs,
        methods_rs,
        warnings,
        operation_count: operations.len(),
        spec_sha256: sha,
    }
}

pub fn generate_pinned() -> Generated {
    generate_from_spec(PINNED_SPEC)
}

/// Normalizes a Rust source for drift comparison: drops comments and all
/// whitespace outside string/char literals. This keeps the checked-in
/// artifacts `cargo fmt`-clean while the drift check stays content-exact
/// (identifiers, string literals, punctuation).
pub fn normalize_ws(src: &str) -> String {
    #[derive(PartialEq)]
    enum State {
        Normal,
        Str,
        Char,
        LineComment,
        BlockComment,
    }
    let mut out = String::with_capacity(src.len());
    let mut state = State::Normal;
    let mut chars = src.chars().peekable();
    let mut prev = '\0';
    while let Some(c) = chars.next() {
        match state {
            State::Normal => match c {
                '"' => {
                    state = State::Str;
                    out.push(c);
                }
                '\'' => {
                    // Lifetime or char literal: char literals in generated
                    // code are extremely rare; treat `'x'` as char only when
                    // a closing quote follows within 3 chars.
                    let mut look = String::new();
                    let mut probe = chars.clone();
                    for _ in 0..3 {
                        match probe.next() {
                            Some(n) => look.push(n),
                            None => break,
                        }
                    }
                    if look.starts_with('\\') || look.chars().nth(1) == Some('\'') {
                        state = State::Char;
                        out.push(c);
                    } else {
                        out.push(c); // lifetime tick, keep
                    }
                }
                '/' if chars.peek() == Some(&'/') => {
                    chars.next();
                    state = State::LineComment;
                }
                '/' if chars.peek() == Some(&'*') => {
                    chars.next();
                    state = State::BlockComment;
                }
                ',' => {
                    // Trailing commas (added/removed by rustfmt when it wraps
                    // argument lists / literals) are semantically inert.
                    let mut probe = chars.clone();
                    for n in probe.by_ref() {
                        if !n.is_whitespace() {
                            if matches!(n, ')' | ']' | '}') {
                                // drop the comma
                            } else {
                                out.push(',');
                            }
                            break;
                        }
                    }
                }
                c if c.is_whitespace() => {}
                c => out.push(c),
            },
            State::Str => {
                out.push(c);
                match c {
                    '\\' => {
                        if let Some(escaped) = chars.next() {
                            out.push(escaped);
                        }
                    }
                    '"' => state = State::Normal,
                    _ => {}
                }
            }
            State::Char => {
                out.push(c);
                match c {
                    '\\' => {
                        if let Some(escaped) = chars.next() {
                            out.push(escaped);
                        }
                    }
                    '\'' => state = State::Normal,
                    _ => {}
                }
            }
            State::LineComment => {
                if c == '\n' {
                    state = State::Normal;
                }
            }
            State::BlockComment => {
                if c == '/' && prev == '*' {
                    state = State::Normal;
                }
            }
        }
        prev = c;
    }
    out
}

/// Canonical output paths relative to the workspace root.
pub fn output_paths() -> [(&'static str, &'static str); 3] {
    [
        ("routes", "crates/oc-server/src/routes.gen.rs"),
        ("types", "crates/oc-client/src/types.gen.rs"),
        ("methods", "crates/oc-client/src/methods.gen.rs"),
    ]
}
