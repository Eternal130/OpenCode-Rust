//! G-247 — the Code Mode sandbox's module-loading/timer contract
//! (bun guest e2e): the tool description's verbatim-ref claim
//! (`tool.ts:64` "Imports, direct filesystem access, and timers are
//! unavailable") is enforced, not just declared —
//!
//! * `import()`/`require` programs reject BEFORE compiling (the
//!   guest's `scanProgramSource`, identical to the oc-core leaf's
//!   `scan_program`) with a fixed error text, as the program's result
//!   (`ok: false`), never a transport error;
//! * legal programs that merely MENTION the words (strings, comments,
//!   regex bodies, property calls) stay runnable — the pinned
//!   false-positive surface of the token-level scan;
//! * `setTimeout`/`setInterval`/`setImmediate` throw for the
//!   program's duration (placeholders swapped onto the globals;
//!   restore rides the capture frame) and the guest's own
//!   soft-timeout keeps arming on the native timer afterwards.

mod common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use serde_json::Value;

use oc_plugin_host::{
    Bridge, CodeModeLimits, CodeModeToolSpec, PluginHost, PluginHostConfig, ToolPort,
};

fn spec(path: &str) -> CodeModeToolSpec {
    CodeModeToolSpec {
        path: path.to_owned(),
        description: String::new(),
        signature: String::new(),
        search_text: String::new(),
    }
}

/// A tool port that answers every nested call with `"ok"`.
struct OkPort;

#[async_trait]
impl ToolPort for OkPort {
    async fn call(&self, _bridge: &Bridge, _name: &str, _input: &Value) -> Result<Value, String> {
        Ok(json!("ok"))
    }
}

/// Minimal block-on for the async facade methods.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(future)
}

fn sandbox_host(sandbox: &std::path::Path) -> PluginHost {
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        tool_port: Some(Arc::new(OkPort)),
        ..PluginHostConfig::default()
    };
    PluginHost::detect().start_detached_with(config, &[])
}

/// `import()`/`require` programs are rejected before they run — the
/// program's error surface, with the scan's fixed text.
#[test]
fn codemode_sandbox_rejects_module_loading() {
    let sandbox = common::sandbox("codemode-sandbox-scan");
    let host = sandbox_host(&sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    let catalog = [spec("greet")];

    let rejected = [
        // The direct escapes the gap entry cites.
        r#"const fs = await import("node:fs"); return fs.readFileSync("/etc/hostname", "utf8");"#,
        r#"return import("node:child_process").then((m) => m.spawn("id"));"#,
        // Whitespace and comment variants of the call shape.
        r#"return import ("node:fs");"#,
        "return import\n(\n'node:fs'\n);",
        r#"return import/*g247*/("node:fs");"#,
        // require — call, bare reference, and the property face
        // (globalThis.require is a live loader on require-capable hosts).
        r#"const fs = require("node:fs"); return fs;"#,
        "const r = require; return typeof r;",
        r#"return globalThis.require && globalThis.require("fs");"#,
        // Hiding inside a template interpolation.
        r#"return `fs: ${import("node:fs")}`;"#,
        r#"return `a ${ `b ${ require("fs") } c` } d`;"#,
    ];
    for code in rejected {
        let run = block_on(host.codemode_evaluate_async(code, &catalog, Duration::from_secs(10)))
            .unwrap_or_else(|_| {
                panic!("rejection is a program result, not a transport error: {code}")
            });
        assert_eq!(
            run.error.as_deref(),
            Some("Code Mode programs cannot use import()/require — the sandbox forbids module loading."),
            "code: {code}"
        );
        assert!(run.value.is_none(), "no value crosses for a rejected program: {code}");
        assert!(run.tool_calls.is_empty(), "no nested calls happen: {code}");
    }

    // The guest is healthy after the rejections.
    let after =
        block_on(host.codemode_evaluate_async("return 6 * 7;", &catalog, Duration::from_secs(5)));
    assert_eq!(after.expect("guest responsive after rejections").value, Some(json!(42)));

    host.shutdown();
}

/// The token-level scan's legal surface: the words inside strings,
/// comments, regex bodies, and property positions stay runnable.
#[test]
fn codemode_sandbox_scan_spares_legal_mentions() {
    let sandbox = common::sandbox("codemode-sandbox-legal");
    let host = sandbox_host(&sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    let catalog = [spec("greet")];

    let legal: &[(&str, serde_json::Value)] = &[
        (
            r#"return "require is just a word: require(\"fs\")";"#,
            json!("require is just a word: require(\"fs\")"),
        ),
        (
            "// import(\"node:fs\") would be rejected — but not inside a comment\nreturn 7;",
            json!(7),
        ),
        ("/* require(\"fs\") */ return 8;", json!(8)),
        // Word boundaries: longer identifiers containing the token.
        (
            "const importer = 1, my_require = 2, required = 3; return importer + my_require + required;",
            json!(6),
        ),
        // Reserved words are legal property names; a property call
        // never loads a module.
        ("const box = { import: (x) => x + 1 }; return box.import(41);", json!(42)),
        // Regex bodies are skipped (operator-position heuristic) and
        // so are the strings they test against.
        (
            r#"const re = /require\(/; return re.test("require(") ? "matched" : "no";"#,
            json!("matched"),
        ),
        // Division reads as division.
        ("const ratio = 84 / 2 / 3; return ratio;", json!(14)),
        // The template BODY (no interpolation) is string content.
        ("return `require(\"fs\") untouchable`;", json!("require(\"fs\") untouchable")),
        // A real nested call still bridges while the words are around.
        (
            r#"const note = "requires a greet"; return [note, await tools.greet({})];"#,
            json!(["requires a greet", "ok"]),
        ),
    ];
    for (code, expected) in legal {
        let run = block_on(host.codemode_evaluate_async(code, &catalog, Duration::from_secs(10)))
            .unwrap_or_else(|_| panic!("legal program must run: {code}"));
        assert!(run.error.is_none(), "code: {code} errored: {:?}", run.error);
        assert_eq!(run.value.as_ref(), Some(expected), "code: {code}");
    }

    host.shutdown();
}

/// Timers throw for the program's duration and the guest's own
/// soft-timeout still arms on the native timer afterwards.
#[test]
fn codemode_sandbox_disables_timers() {
    let sandbox = common::sandbox("codemode-sandbox-timers");
    let host = sandbox_host(&sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    let catalog = [spec("greet")];

    // Each scheduling global throws when CALLED, with the per-name
    // message; the program catches and returns it for the assertion.
    for name in ["setTimeout", "setInterval", "setImmediate"] {
        let code = format!(
            r#"try {{ {name}(() => {{}}, 10); return "scheduled"; }} catch (error) {{ return "threw: " + error.message; }}"#
        );
        let run = block_on(host.codemode_evaluate_async(&code, &catalog, Duration::from_secs(10)))
            .unwrap_or_else(|_| panic!("timer rejection is a program result: {code}"));
        assert_eq!(
            run.value,
            Some(json!(format!(
                "threw: Code Mode programs cannot use {name} — timers are unavailable in the sandbox."
            ))),
            "code: {code}"
        );
    }

    // A bare un-caught call surfaces as the program's error verbatim.
    let run = block_on(host.codemode_evaluate_async(
        "setTimeout(() => {}, 1); return 1;",
        &catalog,
        Duration::from_secs(10),
    ))
    .expect("the throw is the program's error");
    assert_eq!(
        run.error.as_deref(),
        Some("Code Mode programs cannot use setTimeout — timers are unavailable in the sandbox.")
    );

    // The soft timeout (the guest's own race on the NATIVE timer)
    // still arms after the placeholder swaps, and the guest stays
    // responsive — the restore rode the capture frame.
    let hung = block_on(host.codemode_evaluate_limits_async(
        "await new Promise(() => {})",
        &catalog,
        0,
        Duration::from_secs(5),
        &CodeModeLimits { timeout_ms: Some(200), ..CodeModeLimits::default() },
    ));
    let run = hung.expect("the soft timeout is a result");
    assert_eq!(run.error.as_deref(), Some("Execution timed out after 200ms."));

    // Async programs (Promise/microtask flows) are unaffected.
    let after = block_on(host.codemode_evaluate_async(
        "const one = await Promise.resolve(20); return one + 22;",
        &catalog,
        Duration::from_secs(5),
    ));
    assert_eq!(after.expect("promise flows unaffected").value, Some(json!(42)));

    host.shutdown();
}

/// G-334 — the Code Mode sandbox's residual escape faces are closed:
/// no global property holds the raw fetch (the wrapper closure is the
/// only home), dynamic-code faces (eval/Function/AsyncFunction — and
/// the `.constructor` escape hatches) throw for the program's
/// duration, and `Bun`/`process` host globals are throwing facades
/// (the host plumbing rides module-scope natives, so the guest stays
/// alive and responsive throughout).
#[test]
fn codemode_sandbox_closes_escape_faces() {
    let sandbox = common::sandbox("codemode-sandbox-escapes");
    let host = sandbox_host(&sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    let catalog = [spec("greet")];
    let dynamic_suffix: &str = " — dynamic code evaluation is unavailable in the sandbox.";

    // (a) NO global property holds the raw fetch — during a run and
    // between runs alike.
    for code in ["return typeof globalThis.__rawFetch;", "return typeof globalThis.fetch;"] {
        let run = block_on(host.codemode_evaluate_async(code, &catalog, Duration::from_secs(10)))
            .unwrap_or_else(|_| panic!("probe is a program result: {code}"));
        assert!(run.error.is_none(), "code: {code} errored: {:?}", run.error);
    }
    let run = block_on(host.codemode_evaluate_async(
        "return typeof globalThis.__rawFetch;",
        &catalog,
        Duration::from_secs(10),
    ))
    .expect("probe runs");
    assert_eq!(run.value, Some(json!("undefined")), "__rawFetch must not exist");

    // (b) eval / Function / AsyncFunction (globals AND the
    // constructor escape hatches) throw the sandbox rejection message
    // when CALLED or CONSTRUCTED.
    let dynamic_faces: &[(&str, &str)] = &[
        ("eval", r#"eval("return 1");"#),
        ("globalThis.eval", r#"globalThis.eval("return 1");"#),
        ("Function", r#"new Function("return 1");"#),
        ("globalThis.Function", r#"new globalThis.Function("return 1");"#),
        (
            "AsyncFunction via .constructor",
            r#"new (Object.getPrototypeOf(async function () {}).constructor)("return 1");"#,
        ),
        ("Function via .constructor", r#"new (function () {}).constructor("return 1");"#),
    ];
    for (name, call) in dynamic_faces {
        let code =
            format!(r#"try {{ {call}; return "ran"; }} catch (error) {{ return error.message; }}"#);
        let run = block_on(host.codemode_evaluate_async(&code, &catalog, Duration::from_secs(10)))
            .unwrap_or_else(|_| panic!("dynamic-code rejection is a program result: {name}"));
        let message =
            run.value.as_ref().and_then(|value| value.as_str()).unwrap_or_default().to_owned();
        assert!(
            message.ends_with(dynamic_suffix),
            "{name} must throw the sandbox rejection message, got: {message:?}"
        );
    }

    // (c) `Bun` and `process` are throwing facades for the program's
    // duration (any access throws).
    for name in ["Bun", "process"] {
        let code = format!(
            r#"try {{ const probe = {name}.version ?? {name}["version"]; return "read"; }} catch (error) {{ return "threw: " + error.message; }}"#
        );
        let run = block_on(host.codemode_evaluate_async(&code, &catalog, Duration::from_secs(10)))
            .unwrap_or_else(|_| panic!("host-global probe is a program result: {name}"));
        assert_eq!(
            run.value,
            Some(json!(format!(
                "threw: Code Mode programs cannot use {name} — host runtime access is unavailable in the sandbox."
            ))),
            "{name} must be a throwing facade"
        );
    }

    // The guest is healthy afterwards: the restore rode the capture
    // frame, the soft timeout still arms on the native timer, and a
    // later program can still use Function-shaped constructs via the
    // HOST (its own AsyncFunction binding) — i.e. evaluation itself
    // still works.
    let after =
        block_on(host.codemode_evaluate_async("return 6 * 7;", &catalog, Duration::from_secs(5)));
    assert_eq!(after.expect("guest restored after the swaps").value, Some(json!(42)));

    host.shutdown();
}
