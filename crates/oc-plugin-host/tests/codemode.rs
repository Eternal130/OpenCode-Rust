//! Code Mode e2e (wave 4, deliverable 1): a plugin tool registered
//! `codemode: true` (the default) becomes **invisible-direct** — the
//! oc-core snapshot hides it behind the synthesized `execute` tool —
//! and a program `const r = await tools.greet(...); return r` runs in
//! the plugin host's GUEST runtime, its `tools.*` calls bridging back
//! through the port seam into the oc-core registry's full pipeline.
//!
//! The glue here (the [`oc_core::tool::codemode::CodeModeRuntime`] impl
//! over [`PluginHost`], the [`ToolPort`] over the registry, and the
//! plugin-tool executor bridge) is exactly what the integrator
//! (oc-server/app boot) will own — the same pattern as the wave-2
//! `RecordingSessionPort`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use oc_core::location::{
    ToolCallContext, ToolExecute, ToolRegistration, ToolRegistryState, ToolRunFailure,
    ToolRunResult,
};
use oc_core::tool::codemode::{
    self, register_code_mode, shape_request_tools, CallRecord, CodeModeRuntime, RunOutcome,
    ToolCallFn, EXECUTE_NAME,
};
use oc_plugin_host::{Bridge, CodeModeToolSpec, PluginHost, PluginHostConfig, ToolPort};

// ---------------------------------------------------------------------------
// The integrator glue
// ---------------------------------------------------------------------------

/// `CodeModeRuntime` over the guest's `codemode.evaluate`.
struct HostCodeModeRuntime {
    host: Arc<PluginHost>,
    timeout: Duration,
}

#[async_trait]
impl CodeModeRuntime for HostCodeModeRuntime {
    async fn evaluate(
        &self,
        code: &str,
        catalog: &[codemode::CatalogEntry],
        _call: ToolCallFn,
    ) -> RunOutcome {
        let specs: Vec<CodeModeToolSpec> = catalog
            .iter()
            .map(|entry| CodeModeToolSpec {
                path: entry.path.clone(),
                description: entry.description.clone(),
                signature: entry.signature.clone(),
                search_text: entry.search_text.clone(),
            })
            .collect();
        match self.host.codemode_evaluate_async(code, &specs, self.timeout).await {
            Ok(run) => RunOutcome {
                value: run.value,
                error: run.error,
                tool_calls: run
                    .tool_calls
                    .into_iter()
                    .map(|record| CallRecord {
                        tool: record.tool,
                        status: record.status,
                        input: record.input,
                    })
                    .collect(),
                truncated: run.truncated,
                ..RunOutcome::default()
            },
            // The hard timeout (or a dead guest) is the program's error.
            Err(message) => RunOutcome { error: Some(message), ..RunOutcome::default() },
        }
    }
}

/// `ToolPort` over the oc-core registry: a nested `tools.<path>` call
/// resolves the pipeline executor (hooks fire per nested call) and
/// answers with the v2 nested-value semantics (joined text, else null).
struct RegistryToolPort {
    registry: Arc<ToolRegistryState>,
}

#[async_trait]
impl ToolPort for RegistryToolPort {
    async fn call(&self, _bridge: &Bridge, name: &str, input: &Value) -> Result<Value, String> {
        let executor = self
            .registry
            .executor(name)
            .ok_or_else(|| format!("Unknown Code Mode tool: {name}"))?;
        let context = ToolCallContext {
            session_id: oc_schema::id::SessionId("ses_codemode_e2e".into()),
            agent: oc_schema::id::AgentId::parse("build"),
            message_id: String::new(),
            call_id: String::new(),
        };
        match executor.execute_with_context(input, &context).await {
            Ok(result) => {
                let text = result
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        oc_schema::tool::Content::Text { text } => Some(text.as_str()),
                        oc_schema::tool::Content::File { .. } => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(if text.is_empty() { Value::Null } else { Value::String(text) })
            }
            Err(failure) => Err(failure.message().to_string()),
        }
    }
}

/// The plugin-tool executor bridge: `tool.transform` registrations
/// dispatch to the guest's `tool.execute` (the wave-2 TODO, closed).
struct PluginToolExecutor {
    host: Arc<PluginHost>,
    name: String,
    timeout: Duration,
}

#[async_trait]
impl ToolExecute for PluginToolExecutor {
    async fn execute(&self, input: &Value) -> Result<ToolRunResult, ToolRunFailure> {
        match self.host.execute_tool_async(&self.name, input, self.timeout).await {
            Ok(Value::String(text)) => Ok(ToolRunResult {
                content: vec![oc_schema::tool::Content::Text { text }],
                metadata: None,
                output: None,
            }),
            Ok(other) => Ok(ToolRunResult {
                content: vec![oc_schema::tool::Content::Text {
                    text: serde_json::to_string(&other).unwrap_or_else(|_| other.to_string()),
                }],
                metadata: None,
                output: Some(other),
            }),
            Err(message) => Err(ToolRunFailure::Error { message, metadata: None }),
        }
    }
}

/// The minimal oc-core services the pipeline executors carry (the
/// codemode leaves never touch them; hooks fire against an empty
/// registry).
fn tool_services(dir: &std::path::Path) -> Arc<oc_core::tool::ToolServices> {
    struct AllowAll;
    #[async_trait]
    impl oc_core::permission::RuleResolver for AllowAll {
        async fn resolve(
            &self,
            _session: &oc_schema::id::SessionId,
            _agent: Option<&oc_schema::id::AgentId>,
        ) -> Result<
            (Option<oc_schema::permission::Ruleset>, Option<oc_schema::permission::Ruleset>),
            oc_core::permission::PermissionError,
        > {
            Ok((None, None))
        }
    }
    let ids: Arc<dyn oc_util::determinism::IdGen> =
        Arc::new(oc_util::determinism::SequentialIdGen::new());
    let clock: Arc<dyn oc_util::determinism::Clock> =
        Arc::new(oc_util::determinism::FixedClock::epoch());
    let permission = oc_core::permission::Permission::new(
        ids,
        clock,
        Arc::new(oc_core::permission::BroadcastEventSink::new(16)),
        Arc::new(oc_core::permission::MemorySavedGrantStore::new(
            Arc::new(oc_util::determinism::SequentialIdGen::new()),
            Arc::new(oc_util::determinism::FixedClock::epoch()),
        )),
        Arc::new(AllowAll),
        oc_schema::id::ProjectId("proj_codemode_e2e".into()),
        None,
    );
    Arc::new(oc_core::tool::ToolServices {
        directory: dir.to_path_buf(),
        project_directory: dir.to_path_buf(),
        home: dir.to_path_buf(),
        access: oc_core::file_access::FileAccess::new(
            permission.clone(),
            dir.to_path_buf(),
            dir.to_path_buf(),
            dir.to_path_buf(),
        ),
        permission,
        mutation: Arc::new(oc_core::file_mutation::FileMutation::new(
            dir.to_path_buf(),
            dir.to_path_buf(),
        )),
        formatter: Arc::new(oc_core::formatter::NoopFormatter::new()),
        instructions: None,
        db: None,
        bus: None,
    })
}

fn call_context() -> ToolCallContext {
    ToolCallContext {
        session_id: oc_schema::id::SessionId("ses_codemode_e2e".into()),
        agent: oc_schema::id::AgentId::parse("build"),
        message_id: "msg_codemode".into(),
        call_id: "call_codemode".into(),
    }
}

// ---------------------------------------------------------------------------
// The e2e
// ---------------------------------------------------------------------------

#[test]
#[allow(clippy::too_many_lines)] // one sequential end-to-end scenario
fn plugin_tool_becomes_invisible_direct_and_runs_via_execute() {
    let sandbox = common::sandbox("codemode");
    let entry = common::copy_fixture(&sandbox, "codetool.ts");
    let registry = Arc::new(ToolRegistryState::new());
    let hooks = Arc::new(oc_core::hooks::HookRegistry::new());
    let services = tool_services(&sandbox);

    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        session_port: Some(Arc::new(common::RecordingSessionPort::default())),
        tool_port: Some(Arc::new(RegistryToolPort { registry: Arc::clone(&registry) })),
        ..PluginHostConfig::default()
    };
    let host = Arc::new(PluginHost::detect().start_detached_with(config, &[entry]));
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)), "host ready");

    // The materialized plugin tools carry the v2 options verbatim.
    let snapshot = host.registrations();
    let tools: Vec<(String, bool, Option<String>)> = snapshot
        .tools
        .iter()
        .map(|tool| (tool.name.clone(), tool.codemode, tool.namespace.clone()))
        .collect();
    assert_eq!(
        tools,
        vec![
            ("boom".into(), true, None),
            ("direct_probe".into(), false, None),
            ("greet".into(), true, None),
        ],
        "tool.transform options land (codemode default true, false honored)"
    );

    // Register the plugin tools into the oc-core registry (the
    // integrator's sync step), then the Code Mode surface.
    for tool in &snapshot.tools {
        let mut registration =
            ToolRegistration::new(&tool.name, tool.description.clone(), tool.input.clone());
        registration.codemode = tool.codemode;
        registration.namespace = tool.namespace.clone();
        registry.register(registration);
        registry.register_executor(
            tool.name.clone(),
            Arc::new(PluginToolExecutor {
                host: Arc::clone(&host),
                name: tool.name.clone(),
                timeout: Duration::from_secs(5),
            }),
        );
    }
    register_code_mode(
        &registry,
        &hooks,
        &services,
        Arc::new(HostCodeModeRuntime { host: Arc::clone(&host), timeout: Duration::from_secs(10) }),
        Duration::from_secs(10),
    );

    // Invisible-direct: greet/boom hidden, direct_probe + execute kept.
    let shaped = shape_request_tools(registry.snapshot(None), None, true);
    assert_eq!(
        shaped.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>(),
        ["direct_probe", "execute"],
        "code-mode tools are invisible-direct; execute is appended last"
    );
    let catalog = codemode::code_mode_entries(&registry);
    let paths: Vec<&str> = catalog.iter().map(|entry| entry.path.as_str()).collect();
    assert_eq!(paths, ["boom", "greet"], "the catalog lists exactly the code-mode half");
    assert!(
        catalog.iter().any(|entry| entry.signature.starts_with("tools.greet(")),
        "signatures render: {}",
        catalog[1].signature
    );
    // The rendered instruction text is the v2 shape.
    let instructions = codemode::catalog_instructions(&registry);
    assert!(instructions.contains("# Code Mode"), "{instructions}");
    assert!(instructions.contains("- boom (1 tool)"), "{instructions}");
    assert!(instructions.contains("- greet (1 tool)"), "{instructions}");
    assert!(instructions.contains("// Greet someone by name"), "{instructions}");
    assert!(!instructions.contains("direct_probe"), "{instructions}");

    // -- the e2e program: guest runtime → tools.greet → port → pipeline
    let executor = registry.executor(EXECUTE_NAME).expect("execute registered");
    let result = futures_util_block_on(executor.execute_with_context(
        &json!({ "code": "const r = await tools.greet({ name: \"Wave4\" }); return r" }),
        &call_context(),
    ))
    .expect("the program completes");
    assert_eq!(
        result.content,
        vec![oc_schema::tool::Content::Text { text: "hello Wave4".to_string() }],
        "the returned string is the output verbatim"
    );
    let metadata = result.metadata.expect("per-call metadata");
    assert_eq!(
        metadata.get("toolCalls"),
        Some(&json!([{ "tool": "greet", "status": "completed", "input": { "name": "Wave4" } }])),
    );
    assert!(!metadata.contains_key("error"));

    // Promise composition + last-expression formatting (JSON pretty).
    let result = futures_util_block_on(executor.execute_with_context(
        &json!({ "code": "const [a, b] = await Promise.all([tools.greet({ name: 'x' }), tools.greet({ name: 'y' })]); return [a, b];" }),
        &call_context(),
    ))
    .expect("composes");
    let text = match &result.content[0] {
        oc_schema::tool::Content::Text { text } => text.clone(),
        oc_schema::tool::Content::File { .. } => String::new(),
    };
    assert_eq!(text, "[\n  \"hello x\",\n  \"hello y\"\n]", "non-string results pretty-print");

    // A failing tool rejects inside the program: error output, not a
    // tool failure; the call row settles as `error`.
    let result =
        futures_util_block_on(executor.execute_with_context(
            &json!({ "code": "return await tools.boom({});" }),
            &call_context(),
        ))
        .expect("a failed program is output");
    let text = match &result.content[0] {
        oc_schema::tool::Content::Text { text } => text.clone(),
        oc_schema::tool::Content::File { .. } => String::new(),
    };
    assert_eq!(text, "tool kaboom");
    let metadata = result.metadata.expect("metadata");
    assert_eq!(metadata.get("error"), Some(&json!(true)));
    assert_eq!(metadata.get("toolCalls"), Some(&json!([{ "tool": "boom", "status": "error" }])),);

    // Non-catalog tools are NOT callable inside the runtime (undefined).
    let result = futures_util_block_on(executor.execute_with_context(
        &json!({ "code": "return typeof tools.direct_probe;" }),
        &call_context(),
    ))
    .expect("completes");
    let text = match &result.content[0] {
        oc_schema::tool::Content::Text { text } => text.clone(),
        oc_schema::tool::Content::File { .. } => String::new(),
    };
    assert_eq!(text, "undefined");

    // A pending-forever program hits the HARD timeout; the guest stays
    // responsive afterwards (async hang, not a sync wedge).
    let hung =
        host.codemode_evaluate("await new Promise(() => {})", &[], Duration::from_millis(400));
    assert!(hung.is_err(), "the hard timeout bounds the program");
    let after = host.codemode_evaluate(
        "return 1 + 1;",
        &[CodeModeToolSpec {
            path: "greet".into(),
            description: String::new(),
            signature: String::new(),
            search_text: String::new(),
        }],
        Duration::from_secs(5),
    );
    assert_eq!(after.expect("guest responsive after the timeout").value, Some(json!(2)));

    host.shutdown();
}

/// RC-02 — the `search` builtin over the inventory mirrors ref
/// semantics (tool-runtime.ts makeSearch): tokenized ranking over
/// paths, descriptions, and input params; namespace scoping; the
/// exact-path shortcut; the v2 default limit of 10 with paging.
#[test]
fn codemode_search_matches_names_descriptions_and_params() {
    let sandbox = common::sandbox("codemode-search");
    let entry = common::copy_fixture(&sandbox, "helloworld.ts");
    let host = common::host_with(&[entry], &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    let spec = |path: &str, description: &str, params: &str| CodeModeToolSpec {
        path: path.to_string(),
        description: description.to_string(),
        signature: format!("tools.{path}(input): Promise<void>"),
        search_text: oc_core::tool::codemode::search_text(
            path,
            description,
            &json!({ "type": "object", "properties": serde_json::from_str::<Value>(params).expect("params json") }),
        ),
    };
    let catalog = vec![
        spec(
            "fs.read_file",
            "Read a file from disk",
            r#"{ "path": { "type": "string", "description": "The file path to read" } }"#,
        ),
        spec("fs.write_file", "Write a file to disk", r#"{ "path": { "type": "string" } }"#),
        spec(
            "weather.forecast",
            "Tomorrow's umbrella outlook",
            r#"{ "city": { "type": "string", "description": "rain chance by city" } }"#,
        ),
    ];

    let run = |code: &str| {
        host.codemode_evaluate(code, &catalog, Duration::from_secs(5))
            .expect("evaluate")
            .value
            .expect("value")
    };

    // Name hits: exact path ranks first, returned as toolExpression.
    let found = run(r#"return search({ query: "read_file" })"#);
    assert_eq!(found.pointer("/items/0/path"), Some(&json!("tools.fs.read_file")));

    // Description hits: "umbrella" appears only in a description.
    let found = run(r#"return search({ query: "umbrella" })"#);
    assert_eq!(found.pointer("/items/0/path"), Some(&json!("tools.weather.forecast")));

    // Param hits: "rain chance" appears only inside a param description.
    let found = run(r#"return search({ query: "rain chance" })"#);
    assert_eq!(found.pointer("/items/0/path"), Some(&json!("tools.weather.forecast")));

    // Namespace scoping bounds the ranked set (the v2 rank runs over
    // the SCOPED entries — the port's original bug ranked globally).
    let found = run(r#"return search({ query: "file", namespace: "weather" })"#);
    assert_eq!(
        found.pointer("/items"),
        Some(&json!([])),
        "weather scope has no 'file' matches even though fs.* do"
    );

    // The exact-path shortcut accepts the toolExpression form.
    let found = run(r#"return search({ query: "tools.fs.read_file" })"#);
    assert_eq!(found.pointer("/items/0/path"), Some(&json!("tools.fs.read_file")));
    assert_eq!(found.get("remaining"), Some(&json!(0)));

    host.shutdown();
}

/// RC-02 — large-inventory behavior: the v2 default limit of 10 with
/// `remaining`/`next.offset` paging (the port previously defaulted to
/// 25), and plural-form matching ("files" finds `read_file`).
#[test]
fn codemode_search_large_inventory_pages_at_ten() {
    let sandbox = common::sandbox("codemode-search-large");
    let entry = common::copy_fixture(&sandbox, "helloworld.ts");
    let host = common::host_with(&[entry], &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    let catalog: Vec<CodeModeToolSpec> = (0..35)
        .map(|index| {
            let path = format!("bulk.item_{index:02}");
            CodeModeToolSpec {
                path: path.clone(),
                description: "bulk entry".to_string(),
                signature: format!("tools.{path}(input): Promise<void>"),
                search_text: oc_core::tool::codemode::search_text(
                    &path,
                    "bulk entry",
                    &json!({ "type": "object", "properties": {} }),
                ),
            }
        })
        .collect();

    let page = host
        .codemode_evaluate(r#"return search({ query: "bulk" })"#, &catalog, Duration::from_secs(5))
        .expect("evaluate")
        .value
        .expect("value");
    assert_eq!(page.pointer("/items").and_then(Value::as_array).map(Vec::len), Some(10));
    assert_eq!(page.get("remaining"), Some(&json!(25)));
    assert_eq!(page.pointer("/next/offset"), Some(&json!(10)));

    // Page through: the last page is short and `next` settles null.
    let tail = host
        .codemode_evaluate(
            r#"return search({ query: "bulk", offset: 30 })"#,
            &catalog,
            Duration::from_secs(5),
        )
        .expect("evaluate")
        .value
        .expect("value");
    assert_eq!(tail.pointer("/items").and_then(Value::as_array).map(Vec::len), Some(5));
    assert_eq!(tail.get("remaining"), Some(&json!(0)));
    assert_eq!(tail.get("next"), Some(&json!(null)));

    // Plural form ("items") matches the singular path segment
    // (termForms' `s` stripping, tool-runtime.ts:199-204).
    let plural = host
        .codemode_evaluate(r#"return search({ query: "items" })"#, &catalog, Duration::from_secs(5))
        .expect("evaluate")
        .value
        .expect("value");
    assert!(
        !plural.pointer("/items").and_then(Value::as_array).unwrap_or(&vec![]).is_empty(),
        "plural 'items' matches 'item_00' paths"
    );

    host.shutdown();
}

/// The session port answers while the tool engine is exercised (the
/// reader is never starved by the async ports — deadlock rule 1). The
/// prompt rides a plugin command (`ctx.session.prompt` from the guest),
/// the tool call rides the port — both in flight at once.
#[test]
fn session_port_answers_alongside_the_tool_engine() {
    let sandbox = common::sandbox("codemode-session");
    let entry = common::copy_fixture(&sandbox, "codetool.ts");
    let registry = Arc::new(ToolRegistryState::new());
    let recording = Arc::new(common::RecordingSessionPort::default());
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        session_port: Some(recording.clone()),
        tool_port: Some(Arc::new(RegistryToolPort { registry: Arc::clone(&registry) })),
        ..PluginHostConfig::default()
    };
    let host = Arc::new(PluginHost::detect().start_detached_with(config, &[entry]));
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    for tool in &host.registrations().tools {
        registry.register_executor(
            tool.name.clone(),
            Arc::new(PluginToolExecutor {
                host: Arc::clone(&host),
                name: tool.name.clone(),
                timeout: Duration::from_secs(5),
            }),
        );
    }

    // The command (guest → session port) on a worker thread; the tool
    // call (port → guest) on the test thread — both outstanding.
    let command_host = Arc::clone(&host);
    let command = std::thread::spawn(move || {
        command_host.execute_command("codetool.ask", None, Duration::from_secs(5))
    });
    let tool = futures_util_block_on(host.execute_tool_async(
        "greet",
        &json!({ "name": "port" }),
        Duration::from_secs(5),
    ));
    let command = command.join().expect("command thread");
    assert!(
        command.is_ok(),
        "ctx.session.prompt answered while a tool call was in flight: {command:?}"
    );
    assert_eq!(tool.expect("tool via port"), json!("hello port"));
    assert_eq!(
        recording.prompts.lock().unwrap().len(),
        1,
        "the prompt landed in the recording port"
    );
    host.shutdown();
}

/// Minimal block-on for the async facade methods (the supervisor's own
/// runtime handle parks fine from a plain test thread).
fn futures_util_block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(future)
}
