//! `tool/plugin/mcp-resource.ts` port (core-runner-map §4.12) —
//! `list_mcp_resources` / `read_mcp_resource`, namespace `opencode`,
//! Code Mode enabled, permission actions per server and `{server}:{uri}`.

use std::sync::Arc;

use oc_schema::tool::Content;
use serde_json::json;

use crate::location::{
    ToolCallContext, ToolExecute, ToolRegistration, ToolRegistryState, ToolRunFailure,
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
        // TOOL-D1: the declared output is the resource catalog
        // (mcp-resource.ts:27,45-46); the metadata stash stays for wire
        // compatibility.
        Ok(ToolRunResult {
            content: vec![Content::Text { text: "Listed MCP resources".to_string() }],
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

/// Installs both tools (namespace `opencode`, Code Mode on).
pub fn register(registry: &ToolRegistryState, mcp: McpRegistry, permission: Permission) {
    let mut list =
        ToolRegistration::new("list_mcp_resources", LIST_DESCRIPTION, list_input_schema());
    list.namespace = Some("opencode".to_string());
    list.permission = Some(LIST_ACTION.to_string());
    list.codemode = true;
    registry.register(list);
    registry.register_executor(
        "list_mcp_resources",
        Arc::new(ListMcpResourcesTool { registry: mcp.clone(), permission: permission.clone() }),
    );

    let mut read =
        ToolRegistration::new("read_mcp_resource", READ_DESCRIPTION, read_input_schema());
    read.namespace = Some("opencode".to_string());
    read.permission = Some(READ_ACTION.to_string());
    read.codemode = true;
    registry.register(read);
    registry.register_executor(
        "read_mcp_resource",
        Arc::new(ReadMcpResourceTool { registry: mcp, permission }),
    );
}

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

    #[tokio::test]
    async fn mcp_resource_registration_namespacing() {
        let tools = crate::location::ToolRegistryState::new();
        let (registry, permission) = fixture().await;
        register(&tools, registry, permission);
        let names: Vec<String> = tools.list().into_iter().map(|tool| tool.name).collect();
        assert_eq!(names, vec!["list_mcp_resources", "read_mcp_resource"]);
        let read = tools.get("read_mcp_resource").expect("read tool");
        assert_eq!(read.namespace.as_deref(), Some("opencode"));
        assert_eq!(read.permission.as_deref(), Some(READ_ACTION));
        assert!(read.codemode);
    }
}
