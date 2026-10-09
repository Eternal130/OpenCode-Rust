//! Generator invariants: determinism, route/client parity (141/141),
//! checked-in artifact drift, pinned-spec hash, namespace coverage.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
}

fn route_operation_ids(routes_rs: &str) -> BTreeSet<String> {
    routes_rs
        .lines()
        .filter_map(|l| l.trim().strip_prefix("operation_id: "))
        .map(|rest| rest.trim_end_matches(',').trim_matches('"').to_owned())
        .collect()
}

fn client_operation_ids(methods_rs: &str) -> BTreeSet<String> {
    let block = methods_rs
        .split("pub static OPERATION_IDS: &[&str] = &[")
        .nth(1)
        .and_then(|rest| rest.split("];").next())
        .expect("OPERATION_IDS block");
    block
        .lines()
        .filter_map(|l| l.trim().strip_prefix('"'))
        .map(|rest| rest.trim_end_matches("\",").to_owned())
        .collect()
}

#[test]
fn deterministic_output() {
    let a = openapi_gen::generate_pinned();
    let b = openapi_gen::generate_pinned();
    assert_eq!(a.routes_rs, b.routes_rs);
    assert_eq!(a.types_rs, b.types_rs);
    assert_eq!(a.methods_rs, b.methods_rs);
}

#[test]
fn covers_141_operations_with_no_warnings() {
    let generated = openapi_gen::generate_pinned();
    assert_eq!(generated.operation_count, 141);
    assert!(
        generated.warnings.is_empty(),
        "unexpected generator warnings: {:?}",
        generated.warnings
    );
}

#[test]
fn route_client_parity_141() {
    let generated = openapi_gen::generate_pinned();
    let routes = route_operation_ids(&generated.routes_rs);
    let client = client_operation_ids(&generated.methods_rs);
    assert_eq!(routes.len(), 141, "route table must cover 141 ops");
    assert_eq!(client.len(), 141, "client must expose 141 op ids");
    assert_eq!(routes, client, "every emitted route must have a matching client method");
    // And each id is backed by exactly one generated async method.
    let method_count = generated.methods_rs.matches("pub async fn ").count();
    assert_eq!(method_count, 141, "one async fn per operation");
    // Route table rows agree.
    let route_rows = generated.routes_rs.matches("Route {").count() - 1; // minus the struct def
    assert_eq!(route_rows, 141);
}

#[test]
fn checked_in_artifacts_match_regeneration() {
    let generated = openapi_gen::generate_pinned();
    for (text, relative) in [
        (&generated.routes_rs, "crates/oc-server/src/routes.gen.rs"),
        (&generated.types_rs, "crates/oc-client/src/types.gen.rs"),
        (&generated.methods_rs, "crates/oc-client/src/methods.gen.rs"),
    ] {
        let path = repo_path(relative);
        let checked_in = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        // Formatting-insensitive (comments/whitespace dropped), content-exact:
        // the checked-in files stay `cargo fmt`-clean without breaking drift.
        assert_eq!(
            openapi_gen::normalize_ws(&checked_in),
            openapi_gen::normalize_ws(text),
            "{relative} on disk differs from regeneration — run `cargo run -p openapi-gen && cargo fmt`"
        );
    }
}

#[test]
fn normalize_ws_ignores_formatting_only_changes() {
    let a = "mod m { fn f() -> u8 { 1 } } // done";
    let b = "mod m {\n    fn f() -> u8 {\n        1\n    }\n}\n/* x */";
    assert_eq!(openapi_gen::normalize_ws(a), openapi_gen::normalize_ws(b));
    assert_eq!(openapi_gen::normalize_ws(a), "modm{fnf()->u8{1}}");
    // String literal contents are preserved verbatim.
    assert_eq!(
        openapi_gen::normalize_ws(r#"let s = "a b  c";"#),
        openapi_gen::normalize_ws(r#"let s = "a b  c";"#)
    );
    assert_ne!(
        openapi_gen::normalize_ws(r#"let s = "a b";"#),
        openapi_gen::normalize_ws(r#"let s = "ab";"#)
    );
    // Lifetimes survive.
    assert_eq!(openapi_gen::normalize_ws("fn f<'a>(x: &'a u8) {}"), "fnf<'a>(x:&'au8){}");
    // Trailing commas are inert (rustfmt adds/removes them when wrapping).
    assert_eq!(
        openapi_gen::normalize_ws("fn f(&self,\n) -> u8 { v[1, 2,] }"),
        openapi_gen::normalize_ws("fn f(&self) -> u8 { v[1, 2] }")
    );
    // Element-separating commas still matter.
    assert_ne!(
        openapi_gen::normalize_ws("let v = [1, 2];"),
        openapi_gen::normalize_ws("let v = [12];")
    );
}

#[test]
fn pinned_spec_hash_is_embedded_and_disk_copy_matches() {
    let generated = openapi_gen::generate_pinned();
    let expected = "0612418392961ef22f0337ce0e1e520270917665fc85f29c4af1c94d1e1873ee";
    assert_eq!(generated.spec_sha256, expected);
    assert!(generated.routes_rs.contains(expected));
    assert!(generated.types_rs.contains(expected));
    // The on-disk pinned copy is still byte-identical to the compiled pin.
    let disk = std::fs::read(openapi_gen::spec_path()).expect("pinned openapi.json on disk");
    assert_eq!(
        oc_util::hash::sha256_hex(&disk),
        expected,
        "tools/openapi-gen/openapi.json drifted from the pinned hash — re-pin deliberately"
    );
    assert_eq!(oc_util::hash::sha256_hex(openapi_gen::PINNED_SPEC), expected);
}

#[test]
fn namespace_modules_mirror_v2_groups() {
    let generated = openapi_gen::generate_pinned();
    for module in [
        "server",
        "debug",
        "migration",
        "location",
        "agent",
        "plugin",
        "session",
        "message",
        "model",
        "generate",
        "provider",
        "integration",
        "websearch",
        "credential",
        "permission",
        "form",
        "fs",
        "shell",
        "mcp",
        "reference",
        "project",
        "worktree",
        "vcs",
        "config",
        "skill",
        "rpc",
        "event",
        "pty",
        "persistent_pty",
    ] {
        assert!(
            generated.methods_rs.contains(&format!("pub mod {module} {{")),
            "missing namespace module `{module}`"
        );
    }
}

#[test]
fn sse_operations_use_stream_decoder() {
    let generated = openapi_gen::generate_pinned();
    assert!(generated.methods_rs.contains("pub async fn subscribe("));
    assert!(generated.methods_rs.contains("execute_sse"));
    // event.subscribe + session.log stream with JSON-in-string double decode.
    assert_eq!(generated.methods_rs.matches("double-decoded by\n    /// `SseStream`").count(), 2);
}

#[test]
fn wildcard_route_emits_path_capture() {
    let generated = openapi_gen::generate_pinned();
    assert!(generated.routes_rs.contains("\"/api/fs/read/*path\""));
    assert!(generated.methods_rs.contains("\"/api/fs/read/{*path}\""));
}

#[test]
fn envelope_unwrapping_matches_v2_client() {
    let generated = openapi_gen::generate_pinned();
    // v2 unwraps `{ data }`-only envelopes (session.get) but keeps
    // `{ location, data }` envelopes whole (agent.list).
    assert!(generated.methods_rs.contains(".map(|env| env.data)"));
    assert!(generated.types_rs.contains("pub struct AgentListResponse"));
}

#[test]
fn error_envelopes_are_typed() {
    let generated = openapi_gen::generate_pinned();
    for tag in [
        "InvalidRequestErrorEncoded",
        "UnauthorizedErrorEncoded",
        "SessionNotFoundErrorEncoded",
        "UnknownErrorEncoded",
        "ServiceUnavailableErrorEncoded",
    ] {
        assert!(
            generated.types_rs.contains(&format!("pub struct {tag}")),
            "error schema {tag} must be typed"
        );
    }
}
