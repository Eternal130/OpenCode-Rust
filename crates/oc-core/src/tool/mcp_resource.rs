//! `tool/plugin/mcp-resource.ts` port (core-runner-map §4.12) —
//! `list_mcp_resources` / `read_mcp_resource`, namespace `opencode`,
//! Code Mode enabled, permission actions per server and `{server}:{uri}`.

use std::sync::Arc;

use oc_schema::tool::Content;
use serde_json::json;

use crate::location::{
    ToolCallContext, ToolExecute, ToolRegistryState, ToolRunFailure,
    ToolRunResult,
};
use crate::mcp::McpRegistry;
use crate::permission::{AssertInput, Permission};
use oc_schema::permission::Source;

/// `list_mcp_resources` — EXACT description.
pub const LIST_DESCRIPTION: &str = "List documents, records, and other data exposed by MCP servers. Use this when the user refers to something that is not a local file, such as a URI with a custom scheme, then load a match with read_mcp_resource. Each entry names the server to read it from. Templates are resources addressed by a parameter such as a record ID; fill in the uriTemplate placeholders before reading.";

/// `read_mcp_resource` — EXACT description.
pub const READ_DESCRIPTION: &str = "Read one MCP resource by server and URI. Not for local files. Always return the full contents rather than slicing or filtering them; oversized output is truncated automatically and the full content is saved to a file you can read. Images and PDFs are shown to you directly.";

/// Permission action for listing (`opencode_list_mcp_resources`).
pub const LIST_ACTION: &str = "opencode_list_mcp_resources";
/// Permission action for reading (`opencode_read_mcp_resource`).
pub const READ_ACTION: &str = "opencode_read_mcp_resource";

#[must_use]
pub fn list_input_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "server": {
                "type": "string",
                "description": "MCP server name as configured. Omit to list every server."
            }
        }
    })
}

#[must_use]
pub fn read_input_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "server": {
                "type": "string",
                "description": "The server field of the discovered resource."
            },
            "uri": {
                "type": "string",
                "description": "Exact resource URI from discovery, an expanded uriTemplate, or the user."
            }
        },
        "required": ["server", "uri"]
    })
}

/// `list_mcp_resources` (mcp-resource.ts:18-48): listing every server
/// checks each server name so per-server rules still apply.
pub async fn list_resources(
    registry: &McpRegistry,
    permission: &Permission,
    server: Option<&str>,
    context: &ToolCallContext,
) -> Result<serde_json::Value, ToolRunFailure> {
    let servers: Vec<String> = match server {
        Some(server) => vec![server.to_string()],
        None => registry.servers().into_iter().map(|server| server.name).collect(),
    };
    permission
        .assert(AssertInput {
            id: None,
            session_id: context.session_id.clone(),
            agent: Some(context.agent.clone()),
            action: LIST_ACTION.to_string(),
            resources: servers.clone(),
            save: Some(servers),
            metadata: None,
            source: Some(Source::Tool {
                message_id: context.message_id.clone(),
                id: context.call_id.clone(),
            }),
        })
        .await
        .map_err(|error| ToolRunFailure::Error {
            message: permission_message(&error),
            metadata: None,
        })?;
    let catalog = match server {
        None => registry.resource_catalog().await,
        Some(server) => registry.resources(server).await.map_err(|error| {
            ToolRunFailure::Error { message: error.to_string(), metadata: None }
        })?,
    };
    Ok(serde_json::to_value(&catalog).unwrap_or_default())
}

/// `read_mcp_resource` (mcp-resource.ts:49-97): permission resources
/// `{server}:{uri}`, save `{server}:*`; blob image/pdf parts become file
/// content.
pub async fn read_resource(
    registry: &McpRegistry,
    permission: &Permission,
    server: &str,
    uri: &str,
    context: &ToolCallContext,
) -> Result<ToolRunResult, ToolRunFailure> {
    permission
        .assert(AssertInput {
            id: None,
            session_id: context.session_id.clone(),
            agent: Some(context.agent.clone()),
            action: READ_ACTION.to_string(),
            resources: vec![format!("{server}:{uri}")],
            save: Some(vec![format!("{server}:*")]),
            metadata: Some(
                serde_json::from_value(json!({ "server": server, "uri": uri })).unwrap_or_default(),
            ),
            source: Some(Source::Tool {
                message_id: context.message_id.clone(),
                id: context.call_id.clone(),
            }),
        })
        .await
        .map_err(|error| ToolRunFailure::Error {
            message: permission_message(&error),
            metadata: None,
        })?;
    let resource =
        registry.read_resource(server, uri).await.map_err(|error| ToolRunFailure::Error {
            message: format!("Unable to read MCP resource {server}:{uri}: {error}"),
            metadata: None,
        })?;
    let Some(resource) = resource else {
        return Err(ToolRunFailure::Error {
            message: format!(
                "MCP server \"{server}\" is not connected or does not expose resources"
            ),
            metadata: None,
        });
    };
    let content: Vec<Content> = resource
        .contents
        .iter()
        .filter_map(|part| match part {
            oc_schema::mcp::ResourceContentPart::Blob { blob, mime_type, .. } => {
                let mime = mime_type.as_deref()?;
                let show = mime.starts_with("image/") || mime == "application/pdf";
                show.then(|| Content::File {
                    uri: format!("data:{mime};base64,{blob}"),
                    mime: mime.to_string(),
                    name: None,
                })
            }
            oc_schema::mcp::ResourceContentPart::Text { .. } => None,
        })
        .collect();
    let mut metadata = oc_schema::common::Record::default();
    metadata.insert("mcp.output".into(), serde_json::to_value(&resource).unwrap_or_default());
    // TOOL-D1: the declared output is the resource itself
    // (mcp-resource.ts:62,79); the metadata stash stays for wire
    // compatibility.
    Ok(ToolRunResult {
        content,
        metadata: Some(metadata),
        output: Some(serde_json::to_value(&resource).unwrap_or_default()),
    })
}

fn permission_message(error: &crate::permission::PermissionError) -> String {
    match error {
        crate::permission::PermissionError::Declined(_) => {
            "The user declined this tool call".to_string()
        }
        crate::permission::PermissionError::Corrected(corrected) => corrected.feedback.clone(),
        crate::permission::PermissionError::Blocked(blocked) => blocked.to_string(),
        crate::permission::PermissionError::SessionNotFound(id) => {
            format!("session not found: {id}")
        }
        crate::permission::PermissionError::Store(error) => {
            format!("saved grant store failure: {error}")
        }
        crate::permission::PermissionError::RequestNotFound(error) => error.to_string(),
    }
}

/// `list_mcp_resources` executor.
pub struct ListMcpResourcesTool {
    pub registry: McpRegistry,
    pub permission: Permission,
}

#[async_trait::async_trait]
impl ToolExecute for ListMcpResourcesTool {
    async fn execute(&self, _input: &serde_json::Value) -> Result<ToolRunResult, ToolRunFailure> {
        Err(ToolRunFailure::Error {
            message: "list_mcp_resources requires an invocation context".to_string(),
            metadata: None,
        })
    }

    async fn execute_with_context(
        &self,
        input: &serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolRunResult, ToolRunFailure> {
        let server = input.get("server").and_then(serde_json::Value::as_str);
        let output = list_resources(&self.registry, &self.permission, server, context).await?;
        let mut metadata = oc_schema::common::Record::default();
        metadata.insert("mcp.output".into(), output.clone());
        // TOOL-D1 + G-60: the declared output is the resource catalog
        // (mcp-resource.ts:27,45-46); normalizeContent stringifies the
        // output when no explicit content parts exist (runtime.ts:57) —
        // the tool's text IS the full catalog JSON, not a placeholder.
        // The metadata stash stays for wire compatibility.
        Ok(ToolRunResult {
            content: vec![Content::Text {
                text: serde_json::to_string(&output).unwrap_or_default(),
            }],
            metadata: Some(metadata),
            output: Some(output),
        })
    }
}

/// `read_mcp_resources` executor.
pub struct ReadMcpResourceTool {
    pub registry: McpRegistry,
    pub permission: Permission,
}

/// G-191 — the leaf adapters: same executors, now registered through the
/// standard pipeline (before/after hooks, decodeInput, normalizeContent),
/// matching ref mcp-resource.ts:17,49 (`editor.add` → tool.ts:150). The
/// visible fix: a plain-text MCP resource returns `content: []` from the
/// leaf exactly like ref, and the pipeline's normalizeContent fallback
/// turns the stringified `output` into the model-facing text part.
macro_rules! pipeline_leaf {
    ($tool:ty) => {
        #[async_trait::async_trait]
        impl super::ToolExecuteWithContext for $tool {
            fn execute<'a>(
                &'a self,
                _services: &'a super::ToolServices,
                input: &'a serde_json::Value,
                context: &'a ToolCallContext,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<super::ToolOutcome, super::ToolFailure>,
                        > + Send
                        + 'a,
                >,
            > {
                Box::pin(async move {
                    ToolExecute::execute_with_context(self, input, context)
                        .await
                        .map(|result| super::ToolOutcome {
                            content: result.content,
                            metadata: result.metadata,
                            output: result.output,
                        })
                        .map_err(crate::tool::subagent::run_failure_to_failure)
                })
            }
        }
    };
}

pipeline_leaf!(ListMcpResourcesTool);
pipeline_leaf!(ReadMcpResourceTool);

/// Installs both tools (namespace `opencode`, Code Mode on) through the
/// standard pipeline (G-191).
pub fn register(
    registry: &ToolRegistryState,
    hooks: &std::sync::Arc<crate::hooks::HookRegistry>,
    services: &std::sync::Arc<super::ToolServices>,
    mcp: McpRegistry,
    permission: Permission,
) -> Result<(), String> {
    super::register_builtin(
        registry,
        hooks,
        services,
        super::BuiltinTool {
            name: "list_mcp_resources",
            description: LIST_DESCRIPTION.to_string(),
            input: list_input_schema(),
            permission: Some(LIST_ACTION),
            codemode: true,
            namespace: Some("opencode"),
            execute: Arc::new(ListMcpResourcesTool {
                registry: mcp.clone(),
                permission: permission.clone(),
            }),
        },
    )?;
    super::register_builtin(
        registry,
        hooks,
        services,
        super::BuiltinTool {
            name: "read_mcp_resource",
            description: READ_DESCRIPTION.to_string(),
            input: read_input_schema(),
            permission: Some(READ_ACTION),
            codemode: true,
            namespace: Some("opencode"),
            execute: Arc::new(ReadMcpResourceTool { registry: mcp, permission }),
        },
    )
}

#[async_trait::async_trait]
impl ToolExecute for ReadMcpResourceTool {
    async fn execute(&self, _input: &serde_json::Value) -> Result<ToolRunResult, ToolRunFailure> {
        Err(ToolRunFailure::Error {
            message: "read_mcp_resource requires an invocation context".to_string(),
            metadata: None,
        })
    }

    async fn execute_with_context(
        &self,
        input: &serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolRunResult, ToolRunFailure> {
        let server = input.get("server").and_then(serde_json::Value::as_str).unwrap_or_default();
        let uri = input.get("uri").and_then(serde_json::Value::as_str).unwrap_or_default();
        read_resource(&self.registry, &self.permission, server, uri, context).await
    }
}

/// Installs both tools — see the pipeline register above (G-191).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::registry::{McpRegistry, NoCredentials};
    use oc_schema::id::{AgentId, SessionId};
    use oc_schema::mcp::{LocalConfig, ServerConfig};

    async fn fixture() -> (McpRegistry, Permission) {
        let database = crate::database::Database::open_in_memory().await.expect("database");
        let bus = crate::bus::Bus::new(database, crate::bus::BusOptions::deterministic());
        let connector: crate::mcp::Connector = Arc::new(|input| {
            let server = input.server.to_string();
            Box::pin(async move {
                Err(crate::mcp::ConnectFailure::Connect(crate::mcp::ConnectError {
                    server,
                    message: "offline".to_string(),
                }))
            })
        });
        let mut servers = std::collections::BTreeMap::new();
        servers.insert(
            "alpha".to_string(),
            ServerConfig::Local(Box::new(LocalConfig {
                command: vec!["x".to_string()],
                cwd: None,
                environment: None,
                disabled: None,
                codemode: None,
                timeout: None,
                protocol: None,
            })),
        );
        let source = Arc::new(move || servers.clone());
        let registry = McpRegistry::with_connector(bus, connector, Arc::new(NoCredentials))
            .with_config_source(source)
            .with_directory("/tmp");
        registry.reconcile().await;
        // The offline connector settles alpha as failed; reads then report
        // the not-connected shape.
        for _ in 0..200 {
            let failed = registry
                .servers()
                .iter()
                .any(|server| matches!(server.status, oc_schema::mcp::Status::Failed { .. }));
            if failed {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let permission = Permission::new(
            Arc::new(oc_util::determinism::SequentialIdGen::new()),
            Arc::new(oc_util::determinism::FixedClock::epoch()),
            Arc::new(crate::permission::BroadcastEventSink::new(8)),
            Arc::new(crate::permission::saved::MemorySavedGrantStore::new(
                Arc::new(oc_util::determinism::SequentialIdGen::new()),
                Arc::new(oc_util::determinism::FixedClock::epoch()),
            )),
            Arc::new(NoRules),
            oc_schema::id::ProjectId::global(),
            None,
        );
        (registry, permission)
    }

    /// Default-agent allow-all grant (v2 baseline).
    struct NoRules;

    #[async_trait::async_trait]
    impl crate::permission::RuleResolver for NoRules {
        async fn resolve(
            &self,
            _session_id: &SessionId,
            _agent: Option<&AgentId>,
        ) -> Result<
            (Option<oc_schema::permission::Ruleset>, Option<oc_schema::permission::Ruleset>),
            crate::permission::PermissionError,
        > {
            Ok((
                Some(vec![oc_schema::permission::Rule {
                    action: "*".into(),
                    resource: "*".into(),
                    effect: oc_schema::permission::Effect::Allow,
                }]),
                None,
            ))
        }
    }

    fn context(session: &SessionId) -> ToolCallContext {
        ToolCallContext {
            session_id: session.clone(),
            agent: AgentId::parse("build"),
            message_id: "msg_1".to_string(),
            call_id: "call_1".to_string(),
        }
    }

    #[tokio::test]
    async fn mcp_resource_descriptions_and_actions_exact() {
        assert_eq!(LIST_ACTION, "opencode_list_mcp_resources");
        assert_eq!(READ_ACTION, "opencode_read_mcp_resource");
        assert!(LIST_DESCRIPTION
            .starts_with("List documents, records, and other data exposed by MCP servers."));
        assert!(READ_DESCRIPTION.starts_with("Read one MCP resource by server and URI."));
    }

    #[tokio::test]
    async fn mcp_resource_read_unconnected_message_exact() {
        let (registry, permission) = fixture().await;
        let session = SessionId::parse("ses_1").expect("session");
        let error = read_resource(&registry, &permission, "alpha", "mock://x", &context(&session))
            .await
            .expect_err("offline");
        match error {
            ToolRunFailure::Error { message, .. } => assert_eq!(
                message,
                "MCP server \"alpha\" is not connected or does not expose resources"
            ),
            other => panic!("expected error failure, got {other:?}"),
        }
    }

    /// G-60 — normalizeContent (runtime.ts:57): with no explicit content
    /// parts, the tool's text IS the stringified catalog (full JSON, not
    /// a placeholder).
    #[tokio::test]
    async fn list_mcp_resources_content_is_the_catalog_json() {
        let (registry, permission) = fixture().await;
        let session = SessionId::parse("ses_1").expect("session");
        let tool = ListMcpResourcesTool { registry, permission };
        let result = tool
            .execute_with_context(&serde_json::json!({}), &context(&session))
            .await
            .expect("list");
        let [Content::Text { text }] = &result.content[..] else {
            panic!("expected one text part: {:?}", result.content)
        };
        let output = result.output.as_ref().expect("output");
        assert_eq!(text, &serde_json::to_string(output).unwrap(), "text = compact catalog JSON");
        let catalog: serde_json::Value = serde_json::from_str(text).expect("valid JSON text");
        assert!(catalog.get("resources").is_some() && catalog.get("templates").is_some());
    }

    #[tokio::test]
    async fn mcp_resource_registration_namespacing() {
        let harness = crate::tool::tests::harness().await;
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        let (registry, permission) = fixture().await;
        register(&harness.tools, &hooks, &harness.services, registry, permission)
            .expect("register");
        let names: Vec<String> = harness
            .tools
            .list()
            .into_iter()
            .map(|tool| tool.name)
            .filter(|name| name.contains("mcp_resource"))
            .collect();
        assert_eq!(names, vec!["list_mcp_resources", "read_mcp_resource"]);
        let read = harness.tools.get("read_mcp_resource").expect("read tool");
        assert_eq!(read.namespace.as_deref(), Some("opencode"));
        assert_eq!(read.permission.as_deref(), Some(READ_ACTION));
        assert!(read.codemode);
    }

    /// G-191 — the mcp-resource tools run through the unified pipeline
    /// (ref mcp-resource.ts:17,49 register via `editor.add`, i.e.
    /// tool.ts:150 executeTool): `tool.execute.before`/`after` fire.
    #[tokio::test]
    async fn g191_pipeline_hooks_fire_for_mcp_resource_tools() {
        let harness = crate::tool::tests::harness().await;
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        let (registry, permission) = fixture().await;
        register(&harness.tools, &hooks, &harness.services, registry, permission)
            .expect("register");

        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let before_seen = Arc::clone(&seen);
        let scope = crate::hooks::HookScope::new();
        scope.track(
            hooks
                .register_tool_execute_before(
                    crate::hooks::fn_hook(
                        move |event: &mut crate::hooks::ToolExecuteBeforeEvent| {
                            before_seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(format!("before:{}", event.tool));
                            Ok(())
                        },
                    ),
                    crate::hooks::HookOptions::default(),
                )
                .await,
        );
        let after_seen = Arc::clone(&seen);
        scope.track(
            hooks
                .register_tool_execute_after(
                    crate::hooks::fn_hook(
                        move |event: &mut crate::hooks::ToolExecuteAfterEvent| {
                            let phase = match event.outcome {
                                crate::hooks::ToolOutcome::Completed { .. } => "after",
                                crate::hooks::ToolOutcome::Error { .. } => "after-error",
                            };
                            after_seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(format!("{phase}:{}", event.tool));
                            Ok(())
                        },
                    ),
                    crate::hooks::HookOptions::default(),
                )
                .await,
        );

        // Success leg: the list tool completes offline (empty catalog).
        let _ = crate::tool::tests::call(
            &harness.tools,
            "list_mcp_resources",
            serde_json::json!({}),
        )
        .await
        .expect("list");

        // Error leg: an invalid read input surfaces the ref decode
        // message (decodeInput runs in the pipeline, before the leaf).
        let failure = crate::tool::tests::call(
            &harness.tools,
            "read_mcp_resource",
            serde_json::json!({ "server": "alpha" }),
        )
        .await
        .expect_err("uri is required");
        assert!(failure.message().contains("Invalid arguments"), "got: {}", failure.message());

        let observed = seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        assert_eq!(
            observed,
            vec![
                "before:opencode_list_mcp_resources".to_string(),
                "after:opencode_list_mcp_resources".to_string(),
                "before:opencode_read_mcp_resource".to_string(),
            ],
            "hooks observe the namespaced pipeline executor"
        );
    }

    /// G-191 — the visible bug the raw registration caused: a
    /// plain-text MCP resource returns `content: []` from the leaf
    /// (only blob image/pdf parts become content, mcp-resource.ts:81-86)
    /// and the pipeline's normalizeContent fallback turns the declared
    /// `output` into the model-facing text part (runtime.ts
    /// normalizeContent → stringify).
    #[tokio::test]
    async fn g191_plain_text_resource_content_becomes_text_part() {
        let database = crate::database::Database::open_in_memory().await.expect("database");
        let bus = crate::bus::Bus::new(database, crate::bus::BusOptions::deterministic());
        let connector =
            crate::mcp::tests::scripted_connector(|| crate::mcp::tests::Script::new());
        let mut servers = std::collections::BTreeMap::new();
        servers.insert("alpha".to_string(), crate::mcp::tests::local_config());
        let source = Arc::new(move || servers.clone());
        let registry = McpRegistry::with_connector(bus, connector, Arc::new(NoCredentials))
            .with_config_source(source)
            .with_directory("/tmp");
        registry.reconcile().await;
        for _ in 0..400 {
            let connected = registry
                .servers()
                .iter()
                .any(|server| matches!(server.status, oc_schema::mcp::Status::Connected));
            if connected {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let harness = crate::tool::tests::harness().await;
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        let permission = fixture_permission().await;
        register(&harness.tools, &hooks, &harness.services, registry, permission)
            .expect("register");

        let result = crate::tool::tests::call(
            &harness.tools,
            "read_mcp_resource",
            serde_json::json!({ "server": "alpha", "uri": "mock://text/1" }),
        )
        .await
        .expect("read");
        assert_eq!(result.content.len(), 1, "empty content normalized: {:?}", result.content);
        match &result.content[0] {
            Content::Text { text } => {
                assert!(text.contains("resource body"), "stringified output: {text}");
                // The text IS the stringified declared output.
                let output = result.output.as_ref().expect("output");
                assert_eq!(text, &output.to_string());
            }
            other => panic!("expected a text part, got {other:?}"),
        }
    }

    /// The `Permission` half of [`fixture`] (shared with the scripted
    /// connector test above).
    async fn fixture_permission() -> Permission {
        Permission::new(
            Arc::new(oc_util::determinism::SequentialIdGen::new()),
            Arc::new(oc_util::determinism::FixedClock::epoch()),
            Arc::new(crate::permission::BroadcastEventSink::new(8)),
            Arc::new(crate::permission::saved::MemorySavedGrantStore::new(
                Arc::new(oc_util::determinism::SequentialIdGen::new()),
                Arc::new(oc_util::determinism::FixedClock::epoch()),
            )),
            Arc::new(NoRules),
            oc_schema::id::ProjectId::global(),
            None,
        )
    }
}
