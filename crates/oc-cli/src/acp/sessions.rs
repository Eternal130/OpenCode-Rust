//! Session attachments (port of `acp/sessions.ts`): the map of
//! opencode sessions this connection drives, per-attach MCP
//! registration, selection state and the config-option/commands
//! watchers.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value};
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::acp::catalog::{Catalog, Catalogs, ModelRef};
use crate::acp::config_option::{self, Selection};
use crate::acp::error::{classify, AcpError};
use crate::acp::Conn;

/// One supported ACP-provided MCP server (`SupportedMcpServer`,
/// sessions.ts:13): stdio or HTTP (never `sse`/`acp`).
#[derive(Debug, Clone)]
pub struct SupportedMcpServer {
    pub name: String,
    pub kind: McpKind,
}

#[derive(Debug, Clone)]
pub enum McpKind {
    Stdio { command: Vec<String>, environment: Map<String, Value> },
    Http { url: String, headers: Vec<(String, String)> },
}

/// `supportedMcpServers` (service.ts:239-249): reject `sse`/`acp`.
pub fn supported_mcp_servers(
    servers: &[agent_client_protocol::schema::v1::McpServer],
) -> Result<Vec<SupportedMcpServer>, AcpError> {
    let mut supported = Vec::new();
    for server in servers {
        match server {
            agent_client_protocol::schema::v1::McpServer::Stdio(stdio) => {
                let mut command = vec![stdio.command.to_string_lossy().into_owned()];
                command.extend(stdio.args.iter().cloned());
                let environment: Map<String, Value> = stdio
                    .env
                    .iter()
                    .map(|entry| (entry.name.clone(), Value::String(entry.value.clone())))
                    .collect();
                supported.push(SupportedMcpServer {
                    name: stdio.name.clone(),
                    kind: McpKind::Stdio { command, environment },
                });
            }
            agent_client_protocol::schema::v1::McpServer::Http(http) => {
                supported.push(SupportedMcpServer {
                    name: http.name.clone(),
                    kind: McpKind::Http {
                        url: http.url.clone(),
                        headers: http
                            .headers
                            .iter()
                            .map(|header| (header.name.clone(), header.value.clone()))
                            .collect(),
                    },
                });
            }
            _ => {
                // `sse` and `acp` servers are unsupported (service.ts:242).
                return Err(AcpError::InvalidRequest {
                    message: "Only stdio and HTTP MCP servers are supported".to_owned(),
                    field: Some("mcpServers".to_owned()),
                });
            }
        }
    }
    Ok(supported)
}

/// `Attached` (sessions.ts:15-19).
#[derive(Debug, Clone)]
pub struct Attached {
    pub id: String,
    pub cwd: String,
    selection: Arc<RwLock<Selection>>,
    selected_tx: mpsc::UnboundedSender<config_option::Change>,
}

impl Attached {
    pub async fn selection(&self) -> Selection {
        self.selection.read().await.clone()
    }

    async fn apply_patch(&self, patch: &config_option::Change) -> Selection {
        let mut selection = self.selection.write().await;
        match patch {
            config_option::Change::Model(model) => selection.model = Some(model.clone()),
            config_option::Change::Mode(mode) => selection.mode_id = Some(mode.clone()),
        }
        selection.clone()
    }
}

/// `Interface` (sessions.ts:21-32), Rust-shaped.
#[derive(Clone)]
pub struct Sessions {
    client: oc_client::Client,
    entries: Arc<Mutex<HashMap<String, Arc<Attached>>>>,
}

impl Sessions {
    pub fn new(client: oc_client::Client) -> Self {
        Self { client, entries: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub async fn has(&self, session_id: &str) -> bool {
        self.entries.lock().await.contains_key(session_id)
    }

    /// `require` (sessions.ts:173-177).
    pub async fn require(&self, session_id: &str) -> Result<Arc<Attached>, AcpError> {
        self.entries
            .lock()
            .await
            .get(session_id)
            .cloned()
            .ok_or_else(|| AcpError::SessionNotFound { session_id: session_id.to_owned() })
    }

    /// `select` (sessions.ts:184-191): update the selection before
    /// switching so the echoed event is a no-op.
    pub async fn select(
        &self,
        attached: &Attached,
        change: &config_option::Change,
    ) -> Result<(), AcpError> {
        attached.apply_patch(change).await;
        let result = match change {
            config_option::Change::Model(model) => {
                self.client
                    .session()
                    .switch_model(
                        &attached.id,
                        &oc_client::types::SessionSwitchModelBody {
                            model: oc_client::types::ModelRef {
                                provider_id: model.provider_id.clone(),
                                id: model.id.clone(),
                                variant: model.variant.clone(),
                            },
                        },
                    )
                    .await
            }
            config_option::Change::Mode(mode) => {
                self.client
                    .session()
                    .switch_agent(
                        &attached.id,
                        &oc_client::types::SessionSwitchAgentBody { agent: mode.clone() },
                    )
                    .await
            }
        };
        result.map_err(|error| classify(&error))
    }

    /// `attach` (sessions.ts:124-164): swap the entry in, register MCP
    /// servers, and return the initial config options plus the
    /// selection-event receiver. The follow-up watcher
    /// (available-commands/config-option updates after the ACP response
    /// goes out) starts via [`Self::start_watch`].
    pub async fn attach(
        &self,
        session: &oc_client::types::SessionInfo,
        mcp_servers: &[SupportedMcpServer],
        catalogs: &Catalogs,
    ) -> Result<
        (
            Arc<Attached>,
            Vec<agent_client_protocol::schema::v1::SessionConfigOption>,
            mpsc::UnboundedReceiver<config_option::Change>,
        ),
        AcpError,
    > {
        let cwd = session.location.directory.clone();
        let catalog = catalogs.get(&cwd).await?;
        let (selected_tx, selected_rx) = mpsc::unbounded_channel();
        let attached = Arc::new(Attached {
            id: session.id.clone(),
            cwd: cwd.clone(),
            selection: Arc::new(RwLock::new(Selection {
                model: session.model.as_ref().map(|model| ModelRef {
                    provider_id: model.provider_id.clone(),
                    id: model.id.clone(),
                    variant: model.variant.clone(),
                }),
                mode_id: session.agent.clone(),
            })),
            selected_tx,
        });
        // Swap synchronously so concurrent attaches of one ID cannot both
        // keep a slot (sessions.ts:136-139).
        self.entries.lock().await.insert(session.id.clone(), Arc::clone(&attached));
        // registerMcp (sessions.ts:93-112).
        for server in mcp_servers {
            self.register_mcp(&attached, server).await?;
        }
        let options = config_option::config_options(&catalog, &attached.selection().await);
        Ok((attached, options, selected_rx))
    }

    async fn register_mcp(
        &self,
        attached: &Attached,
        server: &SupportedMcpServer,
    ) -> Result<(), AcpError> {
        let config = match &server.kind {
            McpKind::Stdio { command, environment } => {
                serde_json::to_value(oc_client::types::McpLocalConfigEncoded {
                    type_: oc_client::types::McpLocalConfigEncodedType::Local,
                    command: command.clone(),
                    environment: Some(
                        environment
                            .iter()
                            .map(|(name, value)| {
                                (name.clone(), value.as_str().unwrap_or_default().to_owned())
                            })
                            .collect(),
                    ),
                    ..Default::default()
                })
                .unwrap_or(serde_json::Value::Null)
            }
            McpKind::Http { url, headers } => {
                serde_json::to_value(oc_client::types::McpRemoteConfigEncoded {
                    type_: oc_client::types::McpRemoteConfigEncodedType::Remote,
                    url: url.clone(),
                    headers: Some(
                        headers.iter().map(|(name, value)| (name.clone(), value.clone())).collect(),
                    ),
                    oauth: Some(oc_client::types::McpRemoteConfigEncodedOauth::Boolean(false)),
                    ..Default::default()
                })
                .unwrap_or(serde_json::Value::Null)
            }
        };
        let config: oc_client::types::ExperimentalMcpAddBodyConfig = serde_json::from_value(config)
            .map_err(|error| AcpError::ServiceFailure {
                safe_message: format!("mcp config encode failed: {error}"),
                service: Some("mcp".to_owned()),
                error_name: None,
            })?;
        self.client
            .mcp()
            .add(
                &server.name,
                &oc_client::types::ExperimentalMcpAddQuery {
                    location: Some(oc_client::types::QueryLocation {
                        directory: Some(attached.cwd.clone()),
                    }),
                },
                &oc_client::types::ExperimentalMcpAddBody { config },
            )
            .await
            .map_err(|error| classify(&error))?;
        Ok(())
    }

    /// The per-attach watcher (sessions.ts:141-159): once the ACP
    /// response went out, push `available_commands_update`, then follow
    /// catalog reloads and selection events to push
    /// `config_option_update` / commands changes. Selection events
    /// arrive over the shared event feed.
    pub async fn start_watch(
        &self,
        attached: Arc<Attached>,
        conn: Conn,
        catalogs: Arc<Catalogs>,
        selected_rx: mpsc::UnboundedReceiver<config_option::Change>,
    ) {
        if !self.has(&attached.id).await {
            return;
        }
        let Ok(catalog) = catalogs.get(&attached.cwd).await else { return };
        let Ok(mut changes) = catalogs.changes(&attached.cwd).await else { return };
        send_commands(&conn, &attached.id, &catalog);
        let mut last_options = config_option::config_options(&catalog, &attached.selection().await);
        let mut last_commands = catalog.commands.clone();
        let mut selected_rx = selected_rx;
        loop {
            let step = tokio::select! {
                _ = changes.changed() => {
                    if changes.has_changed().is_err() {
                        return;
                    }
                    catalogs.get(&attached.cwd).await
                }
                patch = selected_rx.recv() => {
                    match patch {
                        Some(patch) => {
                            attached.apply_patch(&patch).await;
                            Ok(catalog.clone())
                        }
                        None => return,
                    }
                }
            };
            let Ok(catalog) = step else { return };
            let selection = attached.selection().await;
            let options = config_option::config_options(&catalog, &selection);
            if options != last_options {
                send_config_options(&conn, &attached.id, options.clone());
                last_options = options;
            }
            if catalog.commands != last_commands {
                last_commands = catalog.commands.clone();
                send_commands(&conn, &attached.id, &catalog);
            }
        }
    }

    /// `detach` (sessions.ts:165-168).
    pub async fn detach(&self, session_id: &str) {
        self.entries.lock().await.remove(session_id);
    }

    /// `release` (sessions.ts:169-172): only when this exact attachment
    /// still owns the slot.
    pub async fn release(&self, attached: &Attached) {
        let mut entries = self.entries.lock().await;
        if entries.get(&attached.id).is_some_and(|current| current.id == attached.id) {
            entries.remove(&attached.id);
        }
    }

    /// Feed a selection event (`session.model.selected` /
    /// `session.agent.selected`, sessions.ts:54-73).
    pub async fn selected(&self, session_id: &str, change: config_option::Change) {
        if let Some(entry) = self.entries.lock().await.get(session_id).cloned() {
            let _ = entry.selected_tx.send(change);
        }
    }
}

/// `sendCommands` (sessions.ts:75-79).
fn send_commands(conn: &Conn, session_id: &str, catalog: &Catalog) {
    let commands = config_option::available_commands(catalog);
    let update = agent_client_protocol::schema::v1::SessionUpdate::AvailableCommandsUpdate(
        agent_client_protocol::schema::v1::AvailableCommandsUpdate::new(
            commands
                .into_iter()
                .filter_map(|command| {
                    let name = command.get("name")?.as_str()?.to_owned();
                    let description = command
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    Some(agent_client_protocol::schema::v1::AvailableCommand::new(
                        name,
                        description,
                    ))
                })
                .collect(),
        ),
    );
    let _ = conn.send_notification(agent_client_protocol::schema::v1::SessionNotification::new(
        session_id.to_owned(),
        update,
    ));
}

/// `changed` (sessions.ts:81-91) — the config-option push.
fn send_config_options(
    conn: &Conn,
    session_id: &str,
    options: Vec<agent_client_protocol::schema::v1::SessionConfigOption>,
) {
    let update = agent_client_protocol::schema::v1::SessionUpdate::ConfigOptionUpdate(
        agent_client_protocol::schema::v1::ConfigOptionUpdate::new(options),
    );
    let _ = conn.send_notification(agent_client_protocol::schema::v1::SessionNotification::new(
        session_id.to_owned(),
        update,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{McpServerHttp, McpServerSse, McpServerStdio};

    fn http_server(name: &str) -> agent_client_protocol::schema::v1::McpServer {
        agent_client_protocol::schema::v1::McpServer::Http(McpServerHttp::new(
            name.to_owned(),
            "https://example.test",
        ))
    }

    #[test]
    fn sse_and_acp_servers_are_rejected() {
        let sse = agent_client_protocol::schema::v1::McpServer::Sse(McpServerSse::new(
            "sse",
            "https://example.test/sse",
        ));
        let error = supported_mcp_servers(&[sse]).unwrap_err();
        assert!(
            matches!(&error, AcpError::InvalidRequest { field: Some(field), .. } if field == "mcpServers")
        );
        assert_eq!(error.to_string(), "Only stdio and HTTP MCP servers are supported");
    }

    #[test]
    fn stdio_and_http_servers_pass_through() {
        let stdio = agent_client_protocol::schema::v1::McpServer::Stdio(
            McpServerStdio::new("local", "npx").args(vec!["-y".to_owned(), "server".to_owned()]),
        );
        let servers = supported_mcp_servers(&[http_server("http"), stdio]).unwrap();
        assert_eq!(servers.len(), 2);
        assert!(matches!(servers[0].kind, McpKind::Http { .. }));
        assert!(matches!(
            &servers[1].kind,
            McpKind::Stdio { command, .. } if command == &vec!["npx".to_owned(), "-y".to_owned(), "server".to_owned()]
        ));
    }
}
