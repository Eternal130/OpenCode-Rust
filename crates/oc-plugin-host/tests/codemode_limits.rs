//! Code Mode execution limits e2e (the v2 `ExecutionLimits` parity):
//! the limits object crosses the wire into the guest runtime, which
//! enforces validation (ref-identical `RangeError` text), the tool-call
//! admission cap, the output bound (byte-exact notice), the value
//! depth guard, and the soft `timeoutMs`; a wedged guest (sync loop)
//! trips the transport deadline, the supervisor KILLS + restarts it,
//! and that deliberate kill is exempt from the 3-strikes crash
//! attribution.

mod common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use oc_plugin_host::{
    Bridge, CodeModeLimits, CodeModeToolSpec, PluginHost, PluginHostConfig, PluginState, ToolPort,
};

fn spec(path: &str) -> CodeModeToolSpec {
    CodeModeToolSpec { path: path.to_owned(), description: String::new(), signature: String::new() }
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

fn limits_host(port: bool, sandbox: &std::path::Path) -> PluginHost {
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        tool_port: port.then(|| Arc::new(OkPort) as Arc<dyn ToolPort>),
        ..PluginHostConfig::default()
    };
    PluginHost::detect().start_detached_with(config, &[])
}

#[test]
fn guest_enforces_execution_limits() {
    let sandbox = common::sandbox("codemode-limits");
    let host = limits_host(true, &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    let catalog = [spec("greet")];

    // -- validation: below-minimum values reject with the ref's exact
    //    RangeError text (the guest validates the raw JSON numbers).
    let invalid = block_on(host.codemode_evaluate_limits_async(
        "return 1",
        &catalog,
        0,
        Duration::from_secs(5),
        &CodeModeLimits { timeout_ms: Some(0), ..CodeModeLimits::default() },
    ));
    assert_eq!(
        invalid.unwrap_err(),
        "timeoutMs must be a safe integer greater than or equal to 1."
    );

    // -- maxToolCalls: the admission counter rejects beyond the limit.
    let capped = block_on(host.codemode_evaluate_limits_async(
        "const a = await tools.greet({}); const b = await tools.greet({}); return [a, b];",
        &catalog,
        0,
        Duration::from_secs(5),
        &CodeModeLimits { max_tool_calls: Some(1), ..CodeModeLimits::default() },
    ));
    let run = capped.expect("a capped program is a result, not a transport error");
    assert_eq!(run.error.as_deref(), Some("Execution exceeded its tool-call limit of 1."));
    assert_eq!(run.tool_calls.len(), 1, "exactly the limit was admitted");
    assert_eq!(run.tool_calls[0].status, "completed");

    // maxToolCalls = 0 rejects every call before it crosses the wire.
    let zero = block_on(host.codemode_evaluate_limits_async(
        "return await tools.greet({});",
        &catalog,
        0,
        Duration::from_secs(5),
        &CodeModeLimits { max_tool_calls: Some(0), ..CodeModeLimits::default() },
    ));
    let run = zero.expect("capped at zero is still a result");
    assert_eq!(run.error.as_deref(), Some("Execution exceeded its tool-call limit of 0."));
    assert_eq!(run.tool_calls.len(), 0);

    // -- maxOutputBytes: the returned value is bounded with the exact
    //    notice (serialized `"x"*200` + quotes = 202 bytes; 10 kept).
    let wide = block_on(host.codemode_evaluate_limits_async(
        "return \"x\".repeat(200);",
        &catalog,
        0,
        Duration::from_secs(5),
        &CodeModeLimits { max_output_bytes: Some(10), ..CodeModeLimits::default() },
    ));
    let run = wide.expect("a truncated program still succeeds");
    assert_eq!(
        run.value,
        Some(json!(
            "\"xxxxxxxxx [result truncated: 202 bytes exceeds the 10-byte output limit; return a smaller value]"
        ))
    );
    assert!(run.truncated, "the guest flags truncation for the leaf");

    // Under the budget: verbatim, not flagged.
    let narrow = block_on(host.codemode_evaluate_limits_async(
        "return \"short\";",
        &catalog,
        0,
        Duration::from_secs(5),
        &CodeModeLimits { max_output_bytes: Some(64), ..CodeModeLimits::default() },
    ));
    let run = narrow.expect("under budget succeeds");
    assert_eq!(run.value, Some(json!("short")));
    assert!(!run.truncated);

    // -- MAX_VALUE_DEPTH: the 33rd nesting level cannot cross to the host.
    let deep = block_on(host.codemode_evaluate_limits_async(
        "let v = 1; for (let i = 0; i < 40; i++) v = { a: v }; return v;",
        &catalog,
        0,
        Duration::from_secs(5),
        &CodeModeLimits::default(),
    ));
    let run = deep.expect("a deep value is the program's error");
    assert_eq!(run.error.as_deref(), Some("Value exceeds the maximum depth of 32."));

    // Tool INPUTS crossing guest→host are guarded too.
    let deep_input = block_on(host.codemode_evaluate_limits_async(
        "let v = 1; for (let i = 0; i < 40; i++) v = { a: v }; await tools.greet(v); return 1;",
        &catalog,
        0,
        Duration::from_secs(5),
        &CodeModeLimits::default(),
    ));
    let run = deep_input.expect("a deep input is the program's error");
    assert_eq!(run.error.as_deref(), Some("Value exceeds the maximum depth of 32."));

    // -- timeoutMs (async hang): the guest's soft timeout answers with
    //    the ref's message and the guest stays responsive.
    let hung = block_on(host.codemode_evaluate_limits_async(
        "await new Promise(() => {})",
        &catalog,
        0,
        Duration::from_secs(5),
        &CodeModeLimits { timeout_ms: Some(250), ..CodeModeLimits::default() },
    ));
    let run = hung.expect("the soft timeout is a result");
    assert_eq!(run.error.as_deref(), Some("Execution timed out after 250ms."));
    let after =
        block_on(host.codemode_evaluate_async("return 6 * 7;", &catalog, Duration::from_secs(5)));
    assert_eq!(after.expect("guest responsive after the soft timeout").value, Some(json!(42)));

    host.shutdown();
}

/// A sync wedge (`while (true) {}`) is NOT interruptible in-process:
/// the transport deadline fires and the supervisor kills + restarts
/// the guest. The deliberate kill is EXEMPT from the 3-strikes crash
/// attribution — a plugin that is mid-dispatch (busy marker up) when
/// the kill lands must NOT collect strikes.
#[test]
fn codemode_timeout_kill_restarts_guest_without_strikes() {
    let sandbox = common::sandbox("codemode-limits-kill");
    let sleeper = common::copy_fixture(&sandbox, "slow.ts");
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        // Tight backoff: three kill/restart cycles stay quick.
        restart_backoff_base: Duration::from_millis(60),
        restart_backoff_max: Duration::from_millis(240),
        ..PluginHostConfig::default()
    };
    let host = PluginHost::detect().start_detached_with(config, &[sleeper]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)), "host ready");

    let plugin_state = |host: &PluginHost| {
        host.registrations()
            .plugins
            .iter()
            .find(|status| status.plugin_id == "slow")
            .map(|status| status.state.clone())
    };
    let probe = |host: &PluginHost| {
        block_on(host.codemode_evaluate_async("return 7;", &[], Duration::from_secs(5)))
            .ok()
            .and_then(|run| run.value)
            == Some(json!(7))
    };

    for cycle in 0..3 {
        // Mark the plugin busy: the slow.tick handler sleeps past any
        // dispatch deadline, so `plugin.busy` is up when the kill lands.
        host.tap().publish(oc_plugin_host::TapEvent::new("slow.tick", json!({})));
        std::thread::sleep(Duration::from_millis(400));

        // The wedge: a sync loop blocks the guest's event loop; the
        // limits-aligned transport deadline fires (~timeoutMs + grace)
        // and the supervisor kills the wedged child.
        let wedged = block_on(host.codemode_evaluate_limits_async(
            "while (true) {}",
            &[],
            0,
            Duration::from_secs(5),
            &CodeModeLimits { timeout_ms: Some(150), ..CodeModeLimits::default() },
        ));
        assert!(wedged.is_err(), "cycle {cycle}: the wedged program times out");

        // The guest must come back (killed → restarted → reloaded).
        assert!(
            common::wait_for(Duration::from_secs(20), || probe(&host)),
            "cycle {cycle}: guest responsive again after the codemode kill"
        );
        // No strike: the deliberate kill never disables the plugin.
        let state = plugin_state(&host);
        assert!(
            !matches!(state, Some(PluginState::Disabled { .. })),
            "cycle {cycle}: codemode-timeout kill must not strike (state {state:?})"
        );
    }

    // After three deliberate kills the plugin is still Active — three
    // ATTRIBUTED crashes would have disabled it (max_plugin_crashes=3).
    // The reload pass may still be running: wait for Active.
    assert!(
        common::wait_for(Duration::from_secs(20), || {
            matches!(plugin_state(&host), Some(PluginState::Active))
        }),
        "no strikes accumulated across codemode kills: {:?}",
        plugin_state(&host)
    );
    host.shutdown();
}

/// RB-01: the structural resource caps (interpreter/limits.ts) ported
/// to the guest bridge — MAX_STRING_LENGTH (1 << 24) /
/// MAX_ARRAY_LENGTH (10_000_000) on every string/array crossing
/// host↔guest, MAX_PENDING_PROMISES (10_000) on concurrently pending
/// bridge requests. All error messages are the ref's exact text.
struct CapsPort;

#[async_trait]
impl ToolPort for CapsPort {
    async fn call(&self, _bridge: &Bridge, name: &str, _input: &Value) -> Result<Value, String> {
        match name {
            // An over-cap tool RESULT (host→guest crossing).
            "huge" => Ok(json!("x".repeat((1 << 24) + 1))),
            // Holds bridge requests open so a flood stays pending.
            "slow" => {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                Ok(json!("late"))
            }
            _ => Ok(json!("ok")),
        }
    }
}

#[test]
fn guest_enforces_structural_caps() {
    let sandbox = common::sandbox("codemode-structural-caps");
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        tool_port: Some(Arc::new(CapsPort) as Arc<dyn ToolPort>),
        ..PluginHostConfig::default()
    };
    let host = PluginHost::detect().start_detached_with(config, &[]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    let catalog = [spec("greet"), spec("huge"), spec("slow")];
    let run_code = |code: &str| {
        block_on(host.codemode_evaluate_async(code, &catalog, Duration::from_secs(60)))
            .expect("cap breaches are program results, not transport errors")
    };

    // -- MAX_STRING_LENGTH: an oversized RETURN value cannot cross
    //    guest→host (ref: `Invalid string length`).
    let run = run_code("return \"x\".repeat((1 << 24) + 1);");
    assert_eq!(run.error.as_deref(), Some("Invalid string length"));
    assert!(run.value.is_none());

    // An oversized tool INPUT cannot cross guest→host either.
    let run = run_code("await tools.greet({ s: \"x\".repeat((1 << 24) + 1) }); return 1;");
    assert_eq!(run.error.as_deref(), Some("Invalid string length"));
    assert_eq!(run.tool_calls.len(), 1);
    assert_eq!(run.tool_calls[0].status, "error");

    // An oversized tool RESULT cannot cross host→guest.
    let run = run_code("return await tools.huge({});");
    assert_eq!(run.error.as_deref(), Some("Invalid string length"));

    // Exactly AT the cap crosses fine (boundary is `>`, ref parity).
    let run = run_code("return \"x\".repeat(1 << 24);");
    assert!(run.error.is_none());
    assert_eq!(run.value.as_ref().and_then(Value::as_str).map(str::len), Some(1 << 24));

    // -- MAX_ARRAY_LENGTH: an oversized array return (ref: `Invalid
    //    array length`) — sparse so the test stays cheap.
    let run = run_code("const a = new Array(10000001); a[0] = 1; return a;");
    assert_eq!(run.error.as_deref(), Some("Invalid array length"));

    // At the cap: fine.
    let run = run_code("const a = new Array(10000000); a[0] = 7; return a[0];");
    assert_eq!(run.value, Some(json!(7)));

    // -- MAX_PENDING_PROMISES: a flood of un-awaited bridge requests
    //    rejects the (N+1)-th with the ref's exact message.
    let run = run_code(
        "const ps = []; for (let i = 0; i < 10005; i++) ps.push(tools.slow({})); await Promise.all(ps);",
    );
    assert_eq!(
        run.error.as_deref(),
        Some("Too many pending promises (limit 10000); await promises before creating more.")
    );

    // Normal usage is unaffected: tools roundtrip, ordinary values
    // cross both directions untouched.
    let run = run_code("return [await tools.greet({}), \"x\".repeat(10)];");
    assert!(run.error.is_none());
    assert_eq!(run.value, Some(json!(["ok", "xxxxxxxxxx"])));

    host.shutdown();
}
