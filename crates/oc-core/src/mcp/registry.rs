//! MCP registry — port of `mcp/index.ts` + `tool/mcp.ts`
//! (core-runner-map §5).
//!
//! Owns one `ServerEntry` per configured server with the five-status
//! lifecycle (`pending`/`connected`/`failed`/`needs_auth`/`disabled`), a
//! startup latch so readers never hang, per-server lifecycle serialization
//! plus per-URL startup serialization for shared remote endpoints, watch
//! callbacks publishing `mcp.tools.changed` / `mcp.resources.changed` /
//! `mcp.status.changed` / `mcp.prompts.changed`, config reconciliation
//! (initial async connect, then diff-driven replace/remove), session-expiry
//! recovery with one replay, OAuth integration registration
//! (`mcp_<sha1(name\0url)[0..16]>`) over the credential-store seam, and the
//! MCP → tool-registry bridge with a 100ms debounce and initial flush gate.
//!
//! The connector is injectable: production routes to
//! [`super::client::connect`], tests script connections (including
//! expiries and list-changed pushes) through the same lifecycle.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use oc_schema::id::{IntegrationId, IntegrationMethodId};
use oc_schema::mcp::{
    Resource, ResourceCatalog, ResourceContent, ResourceContentPart, ResourceTemplate,
    ServerConfig, Status,
};
use oc_schema::tool::Content as ToolContent;
use parking_lot::Mutex as SyncMutex;
use rusqlite::OptionalExtension;
use tokio::sync::{Mutex as AsyncMutex, Notify};

use crate::bus::{Bus, PublishOptions};
use crate::database::{Database, DbError};
use crate::location::{
    ToolExecute, ToolRegistration, ToolRegistryState, ToolRunFailure, ToolRunResult,
};

use super::client::{self, ConnectFailure, ConnectInput};

/// `mcp.prompts.changed` (defined in v2's `Mcp` module, published here).
pub const PROMPTS_CHANGED: &str = "mcp.prompts.changed";
/// Default debounce for tool-registry refreshes (tool/mcp.ts:139).
pub const TOOL_REFRESH_DEBOUNCE: Duration = Duration::from_millis(100);

/// `MCP.NotFoundError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("MCP server not found: {server}")]
pub struct NotFoundError {
    pub server: String,
}

/// `MCP.ToolCallError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ToolCallError {
    pub server: String,
    pub tool: String,
    pub message: String,
}

/// Registry errors surfaced to callers.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error(transparent)]
    NotFound(#[from] NotFoundError),
    #[error(transparent)]
    ToolCall(#[from] ToolCallError),
}

/// One tool as the registry exposes it (`Mcp.Tool`): the SDK definition
/// tagged with its server and Code Mode flag.
#[derive(Debug, Clone)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
    pub output_schema: Option<serde_json::Value>,
    pub server: String,
    pub codemode: Option<bool>,
}

impl Tool {
    /// `tool.codemode !== false` (tool/mcp.ts:45).
    #[must_use]
    pub fn codemode_enabled(&self) -> bool {
        self.codemode != Some(false)
    }
}

/// `Mcp.ToolResult`.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub is_error: bool,
    pub structured: Option<serde_json::Value>,
    pub content: Vec<client::CallToolContent>,
    pub server: String,
    pub tool: String,
}

/// `Mcp.ServerInfo`.
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub status: Status,
    pub integration_id: Option<IntegrationId>,
}

/// `Mcp.Prompt`.
#[derive(Debug, Clone)]
pub struct Prompt {
    pub name: String,
    pub description: Option<String>,
    pub server: String,
    /// The prompt's declared argument NAMES, declaration order (G-215:
    /// `server:prompt` commands wire positional input into these).
    pub arguments: Vec<String>,
}

/// `Mcp.ServerInstructions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInstructions {
    pub server: String,
    pub instructions: String,
}

/// Per-server runtime state (`ServerEntry`, index.ts:78-89).
struct ServerEntry {
    config: ServerConfig,
    status: SyncMutex<Status>,
    /// Opened once start/replace settles so `callTool` readers never hang.
    startup: Arc<Notify>,
    startup_open: SyncMutex<bool>,
    client: AsyncMutex<Option<client::Connection>>,
    tools: SyncMutex<Option<Vec<Tool>>>,
    prompts: SyncMutex<Option<Vec<Prompt>>>,
    integration_id: Option<IntegrationId>,
    /// The OAuth method id (`register`'s derived suffix — index.ts:190).
    integration_method_id: Option<IntegrationMethodId>,
}

impl ServerEntry {
    fn new(config: ServerConfig) -> Self {
        Self {
            config,
            status: SyncMutex::new(Status::Pending),
            startup: Arc::new(Notify::new()),
            startup_open: SyncMutex::new(false),
            client: AsyncMutex::new(None),
            tools: SyncMutex::new(None),
            prompts: SyncMutex::new(None),
            integration_id: None,
            integration_method_id: None,
        }
    }

    /// The server name for log lines (v2 keys credentials on name + URL).
    fn integration_label(&self) -> String {
        self.integration_id.as_ref().map_or_else(String::new, |id| id.as_str().to_owned())
    }

    fn open_startup(&self) {
        let mut open = self.startup_open.lock();
        if !*open {
            *open = true;
            self.startup.notify_waiters();
        }
    }

    async fn await_startup(&self) {
        if !*self.startup_open.lock() {
            self.startup.notified().await;
        }
    }

    fn set_status(&self, status: Status) {
        *self.status.lock() = status;
    }
}

/// The connector seam: production routes to `client::connect`; tests
/// script connections.
pub type Connector = Arc<
    dyn Fn(
            ConnectInput<'_>,
        )
            -> futures::future::BoxFuture<'static, Result<client::Connection, ConnectFailure>>
        + Send
        + Sync,
>;

/// Credential-store seam over database `credential` rows: the full
/// OAuth credential of an integration's active row (PLUG-D30 — the
/// connect-time provider's `tokens()`/`clientInformation()` view) plus
/// the rotation save (`saveTokens`). A missing credential plus a 401
/// still settles the server as `needs_auth`; the authorize/refresh flow
/// itself lives in [`super::oauth`].
pub trait CredentialStore: Send + Sync {
    /// The active stored OAuth credential for `integration_id`.
    fn oauth_credential(
        &self,
        integration_id: &IntegrationId,
    ) -> futures::future::BoxFuture<'static, Option<super::oauth::StoredCredential>>;
    /// Persists a rotated token answer onto one credential row.
    fn save_oauth_tokens(
        &self,
        integration_id: &IntegrationId,
        credential_id: &str,
        tokens: &super::oauth::StoredTokens,
    ) -> futures::future::BoxFuture<'static, Result<(), String>>;
    /// G-110③: drops a dead credential row (the ref's
    /// `invalidateCredentials`, oauth.ts:290-296 — the provider's
    /// `invalidate` callback removes the stored sign-in when the token
    /// is rejected, so a stale refresh token stops retrying forever).
    fn remove_credential(
        &self,
        credential_id: &str,
    ) -> futures::future::BoxFuture<'static, Result<(), String>>;
}

/// Database-backed credential store (`credential` rows hold JSON
/// `Credential.Value`; OAuth rows serialize the v2 `Credential.OAuth`
/// fields, with the legacy `accessToken`/`access_token`/`token` shapes
/// tolerated on read).
#[derive(Clone)]
pub struct DatabaseCredentials {
    db: Database,
}

impl DatabaseCredentials {
    #[must_use]
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

/// The active row for an integration: `(id, value JSON)` — v2's
/// `desc(active), desc(time_created), desc(id)` active pick
/// (credential.ts:186-192).
const ACTIVE_ROW_SQL: &str = "SELECT id, value FROM credential WHERE integration_id = ?1 \
     ORDER BY active DESC, time_created DESC, id DESC LIMIT 1";

/// Parses one stored row into the OAuth view (None for non-OAuth or
/// unparsable rows). Three shapes tolerated, newest first: the
/// integration store's `snake_case` `Credential.OAuth`, the HTTP client's
/// camelCase wire form (`methodID`), and hand-written/foreign rows
/// naming any access-token key.
fn parse_row(id: &str, value: Option<String>) -> Option<super::oauth::StoredCredential> {
    let raw = value?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
    if let Ok(stored) =
        serde_json::from_value::<crate::integration::types::OAuthValue>(parsed.clone())
    {
        return Some(super::oauth::stored_credential(id, &stored));
    }
    // The camelCase wire form: remap the field names, keep metadata
    // (the registered client lives in `metadata.client`).
    if parsed.get("methodID").is_some() {
        let mut remapped = parsed.clone();
        if let Some(map) = remapped.as_object_mut() {
            if let Some(method) = map.remove("methodID") {
                map.insert("method_id".to_owned(), method);
            }
        }
        if let Ok(stored) =
            serde_json::from_value::<crate::integration::types::OAuthValue>(remapped)
        {
            return Some(super::oauth::stored_credential(id, &stored));
        }
    }
    // Legacy shapes (hand-written or foreign rows): any access-token key.
    let access = parsed
        .get("access")
        .or_else(|| parsed.get("accessToken"))
        .or_else(|| parsed.get("access_token"))
        .or_else(|| parsed.get("token"))
        .and_then(serde_json::Value::as_str)?
        .to_owned();
    Some(super::oauth::StoredCredential {
        id: id.to_owned(),
        method_id: parsed
            .get("method_id")
            .or_else(|| parsed.get("methodID"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        access,
        refresh: parsed
            .get("refresh")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        expires: parsed.get("expires").and_then(serde_json::Value::as_i64).unwrap_or(0),
        token_type: "Bearer".to_owned(),
        scope: None,
        client: None,
    })
}

impl CredentialStore for DatabaseCredentials {
    fn oauth_credential(
        &self,
        integration_id: &IntegrationId,
    ) -> futures::future::BoxFuture<'static, Option<super::oauth::StoredCredential>> {
        let db = self.db.clone();
        let integration_id = integration_id.as_str().to_string();
        Box::pin(async move {
            let row = db
                .transaction(move |tx| {
                    Ok(tx
                        .conn()
                        .query_row(ACTIVE_ROW_SQL, rusqlite::params![integration_id], |row| {
                            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                        })
                        .optional()?)
                })
                .await
                .ok()
                .flatten()?;
            parse_row(&row.0, row.1)
        })
    }

    fn save_oauth_tokens(
        &self,
        integration_id: &IntegrationId,
        credential_id: &str,
        tokens: &super::oauth::StoredTokens,
    ) -> futures::future::BoxFuture<'static, Result<(), String>> {
        let db = self.db.clone();
        let integration = integration_id.as_str().to_string();
        let credential_id = credential_id.to_owned();
        let tokens = tokens.clone();
        Box::pin(async move {
            // The closure returns `Result<Result<(), String>, DbError>`:
            // flow failures travel as the Ok-inner (the transaction itself
            // only fails on database errors).
            let outcome = db
                .transaction(move |tx| {
                    let value: Option<String> = tx
                        .conn()
                        .query_row(
                            "SELECT value FROM credential WHERE id = ?1 AND integration_id = ?2",
                            rusqlite::params![credential_id, integration],
                            |row| row.get(0),
                        )
                        .optional()?;
                    let Some(raw) = value else {
                        return Ok(Err(format!("credential {credential_id} not found")));
                    };
                    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
                        return Ok(Err(format!("credential {credential_id} is not an OAuth row")));
                    };
                    let Ok(previous) =
                        serde_json::from_value::<crate::integration::types::OAuthValue>(parsed)
                    else {
                        return Ok(Err(format!("credential {credential_id} is not an OAuth row")));
                    };
                    let rotated = super::oauth::value_with_tokens(&previous, &tokens);
                    let encoded = serde_json::to_string(
                        &crate::integration::types::CredentialValue::OAuth(rotated),
                    )
                    .map_err(DbError::Json)?;
                    tx.conn().execute(
                        "UPDATE credential SET value = ?2, time_updated = ?3 WHERE id = ?1",
                        rusqlite::params![
                            credential_id,
                            encoded,
                            crate::database::migrations::now_ms()
                        ],
                    )?;
                    Ok(Ok(()))
                })
                .await;
            match outcome {
                Ok(inner) => inner,
                Err(error) => Err(error.to_string()),
            }
        })
    }

    fn remove_credential(
        &self,
        credential_id: &str,
    ) -> futures::future::BoxFuture<'static, Result<(), String>> {
        let db = self.db.clone();
        let credential_id = credential_id.to_owned();
        Box::pin(async move {
            db.transaction(move |tx| {
                tx.conn().execute(
                    "DELETE FROM credential WHERE id = ?1",
                    rusqlite::params![credential_id],
                )?;
                Ok(())
            })
            .await
            .map_err(|error| error.to_string())
        })
    }
}

/// No credentials available (tests, anonymous servers).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCredentials;

impl CredentialStore for NoCredentials {
    fn oauth_credential(
        &self,
        _integration_id: &IntegrationId,
    ) -> futures::future::BoxFuture<'static, Option<super::oauth::StoredCredential>> {
        Box::pin(async { None })
    }

    fn save_oauth_tokens(
        &self,
        _integration_id: &IntegrationId,
        _credential_id: &str,
        _tokens: &super::oauth::StoredTokens,
    ) -> futures::future::BoxFuture<'static, Result<(), String>> {
        Box::pin(async { Err("no credential store".to_owned()) })
    }

    fn remove_credential(
        &self,
        _credential_id: &str,
    ) -> futures::future::BoxFuture<'static, Result<(), String>> {
        Box::pin(async { Err("no credential store".to_owned()) })
    }
}

/// `Integration.ID.make(suffix)` derivation for remote servers
/// (index.ts:176-190): key identity on name + URL, not URL alone — two
/// configs for the same URL under different names are distinct logical
/// servers that may hold different accounts.
#[must_use]
pub fn integration_suffix(name: &str, url: &str) -> String {
    // git2 exposes SHA-1 through blob hashing: hash(name + "\0" + url),
    // then keep the first 16 hex chars like v2's digest().slice(0, 16).
    let mut payload = Vec::with_capacity(name.len() + 1 + url.len());
    payload.extend_from_slice(name.as_bytes());
    payload.push(0);
    payload.extend_from_slice(url.as_bytes());
    let oid = git2::Oid::hash_object(git2::ObjectType::Blob, &payload)
        .expect("SHA-1 blob hash is infallible");
    let hex = oid.to_string();
    format!("mcp_{}", &hex[..16])
}

/// `unavailable` (index.ts:65-76) — EXACT formats.
#[must_use]
pub fn unavailable(server: &str, status: &Status) -> String {
    match status {
        Status::Failed { error } => {
            format!("MCP server \"{server}\" is not connected: {error}. Reconnect it from /mcps.")
        }
        Status::NeedsAuth { error } => {
            format!("MCP server \"{server}\" needs authentication: {error}. Sign in from /mcps.")
        }
        Status::Disabled => format!("MCP server \"{server}\" is disabled."),
        Status::Pending | Status::Connected => format!("MCP server \"{server}\" is not connected."),
    }
}

struct Inner {
    bus: Bus,
    entries: SyncMutex<BTreeMap<String, Arc<ServerEntry>>>,
    /// Serializes lifecycle operations per server.
    locks: dashmap::DashMap<String, Arc<AsyncMutex<()>>>,
    /// Shared remote endpoints must not receive concurrent startup bursts.
    endpoint_loads: dashmap::DashMap<String, Arc<AsyncMutex<()>>>,
    connector: Connector,
    credentials: Arc<dyn CredentialStore>,
    /// The OAuth flow's HTTP client (PLUG-D30's `loggedFetch` — one
    /// client, the config's headers applied per request).
    http: reqwest::Client,
    /// Applied config snapshot for diffing (reconcile).
    applied: SyncMutex<Option<BTreeMap<String, ServerConfig>>>,
    /// Runtime overrides (`add` ⇒ `Some`, `remove` ⇒ `None`).
    overrides: SyncMutex<BTreeMap<String, Option<ServerConfig>>>,
    reconcile_lock: AsyncMutex<()>,
    /// Config source read by `reconcile` (merged `mcp.servers`).
    config_source: SyncMutex<Arc<dyn Fn() -> BTreeMap<String, ServerConfig> + Send + Sync>>,
    /// Location directory for stdio cwd + roots/list.
    directory: SyncMutex<String>,
    /// The elicitation handler shared by every connection (index.ts:231 —
    /// one handler per location, its pending-URL map keyed server+id).
    elicitation: SyncMutex<Option<Arc<super::elicitation::ClientElicitation>>>,
    /// G-1 residual — the Environment stdio seam stdio servers spawn
    /// through (ref `mcp/stdio.ts` routes spawns through the location's
    /// Environment spawner). Defaults to the local host spawner; the
    /// engine installs the location-bound Environment service's driver.
    stdio: SyncMutex<Arc<dyn crate::environment::stdio::StdioSpawner>>,
}

/// The registry service (`Mcp.Service`). Clone shares all state.
#[derive(Clone)]
pub struct McpRegistry {
    inner: Arc<Inner>,
}

impl McpRegistry {
    /// Assembles the registry with the production connector and a
    /// database-backed credential seam.
    #[must_use]
    pub fn new(bus: Bus, db: Database) -> Self {
        Self::with_connector(bus, production_connector(), Arc::new(DatabaseCredentials::new(db)))
    }

    /// Assembles the registry with an injectable connector (tests).
    #[must_use]
    pub fn with_connector(
        bus: Bus,
        connector: Connector,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self::with_parts(bus, connector, credentials, None)
    }

    /// Full construction with an explicit location directory.
    #[must_use]
    pub fn with_parts(
        bus: Bus,
        connector: Connector,
        credentials: Arc<dyn CredentialStore>,
        directory: Option<String>,
    ) -> Self {
        let directory = directory.unwrap_or_else(|| {
            std::env::current_dir()
                .map(|dir| dir.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let registry = Self {
            inner: Arc::new(Inner {
                bus: bus.clone(),
                entries: SyncMutex::new(BTreeMap::new()),
                locks: dashmap::DashMap::new(),
                endpoint_loads: dashmap::DashMap::new(),
                connector,
                credentials,
                http: oc_http::client(),
                applied: SyncMutex::new(None),
                overrides: SyncMutex::new(BTreeMap::new()),
                reconcile_lock: AsyncMutex::new(()),
                config_source: SyncMutex::new(Arc::new(BTreeMap::new)),
                directory: SyncMutex::new(directory),
                elicitation: SyncMutex::new(None),
                stdio: SyncMutex::new(std::sync::Arc::new(
                    crate::environment::stdio::LocalStdioSpawner,
                )),
            }),
        };
        Self::spawn_credential_reconnect(bus, registry.clone());
        registry
    }

    /// Installs the elicitation handler (v2's layer creates one
    /// `ElicitationHandler` and passes it into every connect,
    /// index.ts:439-446). Connections started before the install keep
    /// their negotiated capabilities; assemble the registry fully before
    /// `reconcile`.
    #[must_use]
    pub fn with_elicitation(
        &self,
        elicitation: Arc<super::elicitation::ClientElicitation>,
    ) -> Self {
        *self.inner.elicitation.lock() = Some(elicitation);
        self.clone()
    }

    /// Installs the Environment stdio spawn seam (G-1 residual): stdio
    /// servers spawn through the location-bound Environment service's
    /// driver instead of a host-bound process.
    #[must_use]
    pub fn with_stdio_spawner(
        &self,
        stdio: Arc<dyn crate::environment::stdio::StdioSpawner>,
    ) -> Self {
        *self.inner.stdio.lock() = stdio;
        self.clone()
    }

    /// PLUG-D30 — the credential-switch reconnect (index.ts:577-599):
    /// bring a server online (or back to `needs_auth`) when its
    /// integration's credential switches, so an OAuth login takes
    /// effect without a restart. Only fires for integrations this
    /// registry registered. Skipped without a live runtime (headless
    /// construction in tests).
    fn spawn_credential_reconnect(bus: Bus, registry: McpRegistry) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        tokio::spawn(async move {
            use futures::StreamExt as _;
            let mut subscription = bus.subscribe();
            while let Some(event) = subscription.next().await {
                if event.type_ != crate::bus::registry::CREDENTIAL_SWITCHED {
                    continue;
                }
                let Some(integration) =
                    event.data.get("integrationID").and_then(serde_json::Value::as_str)
                else {
                    continue;
                };
                let owned: Vec<String> = registry
                    .inner
                    .entries
                    .lock()
                    .iter()
                    .filter(|(_, entry)| {
                        entry.integration_id.as_ref().is_some_and(|id| id.as_str() == integration)
                    })
                    .map(|(name, _)| name.clone())
                    .collect();
                for name in owned {
                    // add()/remove() may have replaced the entry while we
                    // waited for the lock — re-check identity under it.
                    let lock = Self::server_lock(&registry.inner, &name);
                    let _guard = lock.lock().await;
                    let Some(entry) = registry.inner.entries.lock().get(&name).cloned() else {
                        continue;
                    };
                    if entry.integration_id.as_ref().is_none_or(|id| id.as_str() != integration) {
                        continue;
                    }
                    if matches!(*entry.status.lock(), Status::Disabled) {
                        continue;
                    }
                    registry.stop_server(&name, &entry).await;
                    registry.start_server(&name, &entry).await;
                }
            }
        });
    }

    /// Installs the config source (`state.get().servers`): the merged
    /// `mcp.servers` map, runtime overrides applied on top at reconcile.
    #[must_use]
    pub fn with_config_source(
        self,
        source: Arc<dyn Fn() -> BTreeMap<String, ServerConfig> + Send + Sync>,
    ) -> Self {
        *self.inner.config_source.lock() = source;
        self
    }

    /// Overrides the location directory (stdio cwd + roots/list answer).
    #[must_use]
    pub fn with_directory(self, directory: impl Into<String>) -> Self {
        *self.inner.directory.lock() = directory.into();
        self
    }

    fn server_lock(inner: &Inner, name: &str) -> Arc<AsyncMutex<()>> {
        let lock = Arc::new(AsyncMutex::new(()));
        let inserted = inner.locks.entry(name.to_string()).or_insert_with(|| lock);
        Arc::clone(&*inserted)
    }

    async fn publish(&self, type_: &str, server: &str) {
        let def = crate::bus::registry::by_type(type_)
            .cloned()
            .unwrap_or_else(|| crate::bus::registry::EventDefinition::ephemeral(type_));
        let _ = self
            .inner
            .bus
            .publish(&def, serde_json::json!({ "server": server }), PublishOptions::default())
            .await;
    }

    fn require_entry(&self, server: &str) -> Result<Arc<ServerEntry>, NotFoundError> {
        self.inner
            .entries
            .lock()
            .get(server)
            .cloned()
            .ok_or_else(|| NotFoundError { server: server.to_string() })
    }

    // -----------------------------------------------------------------
    // lifecycle
    // -----------------------------------------------------------------

    /// `startServer` (index.ts:427-481): announce pending, connect with the
    /// tool listing inside the startup (a listing failure marks the server
    /// failed rather than connected-with-no-tools), then install watches
    /// and publish the settled status.
    async fn start_server(&self, name: &str, entry: &Arc<ServerEntry>) {
        entry.set_status(Status::Pending);
        self.publish("mcp.status.changed", name).await;

        // PLUG-D30 — `connectProvider` (index.ts:223-229 + oauth.ts
        // 272-328): the integration's active credential feeds the bearer;
        // a token inside the refresh margin rotates first (the SDK
        // provider's `tokens()` contract).
        let bearer = match &entry.integration_id {
            Some(id) => self.connect_bearer(entry, id).await,
            None => None,
        };
        let directory = self.inner.directory.lock().clone();
        let elicitation = self.inner.elicitation.lock().clone();
        let stdio = self.inner.stdio.lock().clone();
        let connect = {
            let connector = Arc::clone(&self.inner.connector);
            let server = name.to_string();
            let config = entry.config.clone();
            let directory = directory.clone();
            async move {
                connector(ConnectInput {
                    server: &server,
                    config: &config,
                    directory: &directory,
                    bearer,
                    elicitation,
                    stdio: Some(stdio),
                })
                .await
            }
        };
        let load = async {
            let connection = connect.await?;
            let tools = connection.tools().await.map_err(|error| {
                ConnectFailure::Connect(client::ConnectError {
                    server: name.to_string(),
                    message: error.message,
                })
            })?;
            Ok::<_, ConnectFailure>((connection, tools))
        };
        let result = match &entry.config {
            ServerConfig::Remote(remote) => {
                let lock = Self::endpoint_lock(&self.inner, &remote.url);
                let _guard = lock.lock().await;
                load.await
            }
            ServerConfig::Local(_) => load.await,
        };

        match result {
            Ok((connection, tools)) => {
                let tagged = tools
                    .iter()
                    .map(|tool| tag_tool(name, &entry.config, tool))
                    .collect::<Vec<_>>();
                *entry.tools.lock() = Some(tagged);
                *entry.prompts.lock() = Some(Vec::new());
                entry.set_status(Status::Connected);
                self.watch(name, entry, &connection);
                *entry.client.lock().await = Some(connection);
                // The tool registry reads on these events; a late-connecting
                // server has no other way to appear.
                self.publish("mcp.tools.changed", name).await;
                self.publish("mcp.resources.changed", name).await;
                self.publish("mcp.status.changed", name).await;
                self.refresh_prompts(name, entry).await;
            }
            Err(failure) => {
                entry.set_status(match failure {
                    ConnectFailure::NeedsAuth(auth) => Status::NeedsAuth { error: auth.message },
                    ConnectFailure::Connect(error) => Status::Failed { error: error.message },
                });
                self.publish("mcp.status.changed", name).await;
            }
        }
        entry.open_startup();
    }

    fn endpoint_lock(inner: &Inner, url: &str) -> Arc<AsyncMutex<()>> {
        let lock = Arc::new(AsyncMutex::new(()));
        let inserted = inner.endpoint_loads.entry(url.to_string()).or_insert_with(|| lock);
        Arc::clone(&*inserted)
    }

    /// PLUG-D30: the connect-time bearer over the integration's active
    /// credential — fresh tokens pass through, tokens inside the 5-minute
    /// refresh margin (or past expiry) rotate through the authorization
    /// server's refresh grant with the rotation persisted. A missing or
    /// dead credential yields `None`: the anonymous connect then meets
    /// the 401 and settles `needs_auth` (v2's connect-without-login).
    async fn connect_bearer(
        &self,
        entry: &Arc<ServerEntry>,
        integration_id: &IntegrationId,
    ) -> Option<String> {
        let stored = self.inner.credentials.oauth_credential(integration_id).await?;
        let ServerConfig::Remote(remote) = &entry.config else {
            return Some(stored.access);
        };
        match super::oauth::prepare_token(&self.inner.http, remote, &stored).await {
            Ok(super::oauth::PreparedToken::Fresh { access }) => Some(access),
            Ok(super::oauth::PreparedToken::Refreshed { access, tokens }) => {
                if let Err(error) = self
                    .inner
                    .credentials
                    .save_oauth_tokens(integration_id, &stored.id, &tokens)
                    .await
                {
                    eprintln!(
                        "opencode mcp: token rotation for \"{}\" could not be stored: {error}",
                        entry.integration_label()
                    );
                }
                Some(access)
            }
            Err(error) => {
                // The stored sign-in is dead (refresh rejected): surface
                // nothing so the connect meets the 401 honestly.
                eprintln!(
                    "opencode mcp: credential refresh for \"{}\" failed: {error}",
                    entry.integration_label()
                );
                // G-110③: a REJECTED grant invalidates the credential (ref
                // oauth.ts:290-296 — `credentials.remove(id)`); without the
                // removal the dead refresh token retries forever.
                // Discovery/network failures are not invalidations.
                if super::oauth::is_token_rejected(&error) {
                    if let Err(remove_error) =
                        self.inner.credentials.remove_credential(&stored.id).await
                    {
                        eprintln!(
                            "opencode mcp: could not remove the dead credential for \"{}\": {remove_error}",
                            entry.integration_label()
                        );
                    }
                }
                None
            }
        }
    }

    /// `stopServer` (index.ts:483-494): drop the connection and publish the
    /// catalog changes so the tool registry clears the server's tools.
    async fn stop_server(&self, name: &str, entry: &Arc<ServerEntry>) {
        let connection = entry.client.lock().await.take();
        *entry.tools.lock() = None;
        *entry.prompts.lock() = None;
        if let Some(connection) = connection {
            connection.close().await;
        }
        self.publish("mcp.tools.changed", name).await;
        self.publish("mcp.resources.changed", name).await;
        self.publish(PROMPTS_CHANGED, name).await;
    }

    /// `register` (index.ts:176-215): remote servers with OAuth not
    /// disabled get an integration identity so credentials live in the
    /// global `credential` store rather than committed config. The
    /// identity keys every credential row the OAuth flow writes
    /// (PLUG-D30: `mcp auth` saves under exactly this id) and the
    /// credential-switch reconnect below matches on it; the CLI-driven
    /// authorize flow itself lives in [`super::oauth`].
    #[allow(clippy::unused_self)] // service-level seam (the integration row's method registration is the CLI lane's)
    fn register_integration(&self, name: &str, entry: &mut ServerEntry) {
        let ServerConfig::Remote(remote) = &entry.config else { return };
        if remote.oauth == Some(oc_schema::mcp::OAuthValue::Disabled) {
            return;
        }
        let suffix = integration_suffix(name, &remote.url);
        // The integration id AND the method id share the derived suffix
        // (v2 registers `method: { id: methodID, type: "oauth", label:
        // name }` — index.ts:190-204); the credential rows the flow
        // stores carry the method id for the refresh grant's public
        // client fallback.
        entry.integration_id = Some(IntegrationId(suffix.clone()));
        entry.integration_method_id = Some(IntegrationMethodId(suffix));
    }

    /// `disposeServer`.
    async fn dispose_server(&self, name: &str, entry: &Arc<ServerEntry>) {
        self.stop_server(name, entry).await;
    }

    /// `replaceServer`.
    async fn replace_server(&self, name: &str, config: ServerConfig) {
        let previous = self.inner.entries.lock().get(name).cloned();
        if let Some(previous) = previous {
            self.dispose_server(name, &previous).await;
        }
        let mut entry = Arc::new(ServerEntry::new(config));
        let entry_slot = Arc::get_mut(&mut entry).expect("freshly created entry");
        self.register_integration(name, entry_slot);
        self.inner.entries.lock().insert(name.to_string(), Arc::clone(&entry));
        if config_disabled(&entry.config) {
            entry.set_status(Status::Disabled);
            entry.open_startup();
            self.publish("mcp.status.changed", name).await;
            return;
        }
        self.start_server(name, &entry).await;
    }

    /// `removeServer`.
    async fn remove_server(&self, name: &str) {
        let Some(entry) = self.inner.entries.lock().get(name).cloned() else { return };
        self.dispose_server(name, &entry).await;
        // Credentials are keyed by name + URL and intentionally survive
        // removal for a later re-add.
        self.inner.entries.lock().remove(name);
        self.publish("mcp.status.changed", name).await;
    }

    // -----------------------------------------------------------------
    // watches
    // -----------------------------------------------------------------

    /// `watch` (index.ts:402-425): close ⇒ failed + stop + status; list
    /// changes refresh the catalogs and publish. Late callbacks from a
    /// replaced connection are dropped (`entry.client` identity check in
    /// [`Self::when_live`]).
    fn watch(&self, name: &str, entry: &Arc<ServerEntry>, connection: &client::Connection) {
        let changed = client::Changed {
            tools: Some(Self::list_changed_callback(
                self.clone(),
                name,
                Arc::clone(entry),
                connection.clone(),
                WatchKind::Tools,
            )),
            prompts: Some(Self::list_changed_callback(
                self.clone(),
                name,
                Arc::clone(entry),
                connection.clone(),
                WatchKind::Prompts,
            )),
            resources: Some(Box::new({
                let registry = self.clone();
                let name = name.to_string();
                move || {
                    let registry = registry.clone();
                    let name = name.clone();
                    tokio::spawn(async move {
                        registry.publish("mcp.resources.changed", &name).await;
                    });
                }
            })),
            // `onSessionExpired(() => fork(recover(name, entry, connection)))`
            // (index.ts:415): background refreshes (list-changed) can
            // observe the expiry too; they do not retry, so reconnect
            // here for them. Foreground calls reconnect through
            // `recovering` (`replay_after_recovery`).
            session_expired: Some(Box::new({
                let registry = self.clone();
                let name = name.to_string();
                let entry = Arc::clone(entry);
                let expired = connection.clone();
                move || {
                    let registry = registry.clone();
                    let name = name.clone();
                    let entry = Arc::clone(&entry);
                    let expired = expired.clone();
                    tokio::spawn(async move {
                        let lock = Self::server_lock(&registry.inner, &name);
                        let _guard = lock.lock().await;
                        let live = entry.client.lock().await;
                        if !live.as_ref().is_some_and(|current| current.is_same(&expired)) {
                            return;
                        }
                        drop(live);
                        // `recover` (index.ts:343-350): stop + start under
                        // the server lock; the replacement's own watch
                        // re-registers and `start_server`'s load
                        // repopulates the tool table.
                        registry.stop_server(&name, &entry).await;
                        registry.start_server(&name, &entry).await;
                    });
                }
            })),
        };
        connection.watch(changed);
        // Close watcher: mark failed, stop, publish — only while this
        // connection is still the entry's live client.
        let registry = self.clone();
        let name = name.to_string();
        let entry = Arc::clone(entry);
        let watcher = connection.clone();
        tokio::spawn(async move {
            watcher.watch_close().await;
            let lock = Self::server_lock(&registry.inner, &name);
            let _guard = lock.lock().await;
            let live = entry.client.lock().await;
            if !live.as_ref().is_some_and(|current| current.is_same(&watcher)) {
                return;
            }
            drop(live);
            entry.set_status(Status::Failed { error: "Connection closed".to_string() });
            registry.stop_server(&name, &entry).await;
            registry.publish("mcp.status.changed", &name).await;
        });
    }

    fn list_changed_callback(
        registry: McpRegistry,
        name: &str,
        entry: Arc<ServerEntry>,
        connection: client::Connection,
        kind: WatchKind,
    ) -> Box<dyn Fn() + Send + Sync> {
        let registry = registry;
        let name = name.to_string();
        Box::new(move || {
            let registry = registry.clone();
            let name = name.clone();
            let entry = Arc::clone(&entry);
            let connection = connection.clone();
            tokio::spawn(async move {
                let name = name.clone();
                let entry = Arc::clone(&entry);
                let connection = connection.clone();
                registry
                    .when_live(&name, &entry, &connection, kind, |registry| {
                        let name = name.clone();
                        let entry = Arc::clone(&entry);
                        let connection = connection.clone();
                        async move {
                            match kind {
                                WatchKind::Tools => {
                                    registry.refresh_tools(&name, &entry, &connection).await;
                                    registry.publish("mcp.tools.changed", &name).await;
                                }
                                WatchKind::Prompts => {
                                    registry.refresh_prompts(&name, &entry).await;
                                }
                            }
                        }
                    })
                    .await;
            });
        })
    }

    /// `whenLive` (index.ts:328-336): run under the server lock only while
    /// the connection is still the entry's live client, so late SDK
    /// callbacks cannot commit obsolete state.
    async fn when_live<F, Fut>(
        &self,
        name: &str,
        entry: &Arc<ServerEntry>,
        connection: &client::Connection,
        kind: WatchKind,
        effect: F,
    ) where
        F: FnOnce(McpRegistry) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let lock = Self::server_lock(&self.inner, name);
        let _guard = lock.lock().await;
        {
            let live = entry.client.lock().await;
            if !live.as_ref().is_some_and(|current| current.is_same(connection)) {
                return;
            }
        }
        let _ = kind;
        effect(self.clone()).await;
    }

    async fn refresh_tools(
        &self,
        name: &str,
        entry: &Arc<ServerEntry>,
        connection: &client::Connection,
    ) {
        if let Ok(tools) = connection.tools().await {
            *entry.tools.lock() =
                Some(tools.iter().map(|tool| tag_tool(name, &entry.config, tool)).collect());
        }
    }

    async fn refresh_prompts(&self, name: &str, entry: &Arc<ServerEntry>) {
        let Some(connection) = entry.client.lock().await.clone() else { return };
        let prompts = connection.prompts().await.unwrap_or_default();
        *entry.prompts.lock() = Some(
            prompts
                .iter()
                .map(|prompt| Prompt {
                    name: prompt.name.clone(),
                    description: prompt.description.as_ref().map(ToString::to_string),
                    server: name.to_string(),
                    arguments: prompt
                        .arguments
                        .iter()
                        .flatten()
                        .map(|argument| argument.name.clone())
                        .collect(),
                })
                .collect(),
        );
        self.publish(PROMPTS_CHANGED, name).await;
    }

    // -----------------------------------------------------------------
    // reconcile
    // -----------------------------------------------------------------

    /// `reconcile` (index.ts:537-575). The initial pass seeds every server
    /// and connects asynchronously (one slow server does not block
    /// startup); later passes diff against `applied` and replace/remove
    /// only what changed.
    pub async fn reconcile(&self) {
        let _guard = self.inner.reconcile_lock.lock().await;
        let mut servers = (self.inner.config_source.lock())();
        for (name, config) in self.inner.overrides.lock().iter() {
            match config {
                Some(config) => {
                    servers.insert(name.clone(), config.clone());
                }
                None => {
                    servers.remove(name);
                }
            }
        }
        let applied = self.inner.applied.lock().take();
        match applied {
            None if self.inner.entries.lock().is_empty() => {
                for (name, config) in &servers {
                    let mut entry = Arc::new(ServerEntry::new(config.clone()));
                    let entry_slot = Arc::get_mut(&mut entry).expect("freshly created entry");
                    self.register_integration(name, entry_slot);
                    self.inner.entries.lock().insert(name.clone(), Arc::clone(&entry));
                }
                *self.inner.applied.lock() = Some(servers.clone());
                let seeded: Vec<(String, Arc<ServerEntry>)> = self
                    .inner
                    .entries
                    .lock()
                    .iter()
                    .map(|(name, entry)| (name.clone(), Arc::clone(entry)))
                    .collect();
                for (name, entry) in seeded {
                    if config_disabled(&entry.config) {
                        entry.set_status(Status::Disabled);
                        entry.open_startup();
                        self.publish("mcp.status.changed", &name).await;
                        continue;
                    }
                    // Initial connections stay asynchronous so one slow
                    // server does not block location startup.
                    let registry = self.clone();
                    tokio::spawn(async move {
                        let lock = Self::server_lock(&registry.inner, &name);
                        let _guard = lock.lock().await;
                        registry.start_server(&name, &entry).await;
                    });
                }
            }
            applied => {
                let applied = applied.unwrap_or_default();
                let mut names: Vec<&String> = applied.keys().chain(servers.keys()).collect();
                names.sort();
                names.dedup();
                for name in names {
                    let previous = applied.get(name);
                    let updated = servers.get(name);
                    if previous.is_some_and(|previous| Some(previous) == updated) {
                        continue;
                    }
                    let lock = Self::server_lock(&self.inner, name);
                    let _guard = lock.lock().await;
                    match updated {
                        None => self.remove_server(name).await,
                        Some(config) => self.replace_server(name, config.clone()).await,
                    }
                }
                *self.inner.applied.lock() = Some(servers);
            }
        }
    }

    /// `Mcp.add`: runtime override + reconcile.
    pub async fn add(&self, server: &str, config: ServerConfig) {
        self.inner.overrides.lock().insert(server.to_string(), Some(config));
        self.reconcile().await;
    }

    /// `Mcp.connect`: stop + start under the server lock.
    pub async fn connect(&self, server: &str) -> Result<(), McpError> {
        let entry = self.require_entry(server)?;
        let lock = Self::server_lock(&self.inner, server);
        let _guard = lock.lock().await;
        self.stop_server(server, &entry).await;
        self.start_server(server, &entry).await;
        Ok(())
    }

    /// `Mcp.disconnect`: stop + mark disabled.
    pub async fn disconnect(&self, server: &str) -> Result<(), McpError> {
        let entry = self.require_entry(server)?;
        let lock = Self::server_lock(&self.inner, server);
        let _guard = lock.lock().await;
        self.stop_server(server, &entry).await;
        entry.set_status(Status::Disabled);
        self.publish("mcp.status.changed", server).await;
        Ok(())
    }

    /// `Mcp.remove`: override tombstone + reconcile.
    pub async fn remove(&self, server: &str) -> Result<(), McpError> {
        self.require_entry(server)?;
        self.inner.overrides.lock().insert(server.to_string(), None);
        self.reconcile().await;
        Ok(())
    }

    // -----------------------------------------------------------------
    // reads
    // -----------------------------------------------------------------

    /// `Mcp.servers` — `localeCompare` name order (index.ts:632-637;
    /// RD-18: the collation, not the `BTreeMap` byte order, decides —
    /// non-ASCII names interleave like the ref).
    #[must_use]
    pub fn servers(&self) -> Vec<ServerInfo> {
        let mut servers: Vec<ServerInfo> = self
            .inner
            .entries
            .lock()
            .iter()
            .map(|(name, entry)| ServerInfo {
                name: name.clone(),
                status: entry.status.lock().clone(),
                integration_id: entry.integration_id.clone(),
            })
            .collect();
        servers.sort_by(|a, b| oc_util::localecmp::locale_cmp(&a.name, &b.name));
        servers
    }

    /// `Mcp.tools` — reads report what is connected now, sorted by
    /// (server, name).
    #[must_use]
    pub fn tools(&self) -> Vec<Tool> {
        let mut tools: Vec<Tool> = self
            .inner
            .entries
            .lock()
            .values()
            .flat_map(|entry| entry.tools.lock().clone().unwrap_or_default())
            .collect();
        tools.sort_by(|a, b| {
            oc_util::localecmp::locale_cmp(&a.server, &b.server)
                .then_with(|| oc_util::localecmp::locale_cmp(&a.name, &b.name))
        });
        tools
    }

    /// `Mcp.callTool` (index.ts:669-691): await startup, unavailable
    /// message when not connected, one expiry recovery + replay.
    pub async fn call_tool(
        &self,
        server: &str,
        name: &str,
        args: Option<serde_json::Map<String, serde_json::Value>>,
        session_id: Option<&str>,
    ) -> Result<ToolResult, McpError> {
        let entry = self.require_entry(server)?;
        entry.await_startup().await;
        let connection = entry.client.lock().await.clone();
        let Some(connection) = connection else {
            return Err(ToolCallError {
                server: server.to_string(),
                tool: name.to_string(),
                message: unavailable(server, &entry.status.lock()),
            }
            .into());
        };
        let result = connection.call_tool(name, args.as_ref(), session_id).await;
        let result = match result {
            Err(error) if error.is_session_expired() => {
                let args = args.clone();
                self.replay_after_recovery(server, &entry, &connection, move |connection| {
                    let args = args.clone();
                    async move { connection.call_tool(name, args.as_ref(), session_id).await }
                })
                .await
                .unwrap_or(Err(error))
            }
            // G-359 — runtime 401/403: the SDK's OAuth provider would
            // auto-refresh here; this port refreshes the stored grant and
            // reconnects once (see [`Self::replay_after_unauthorized`]).
            Err(error) if error.is_unauthorized() => self
                .replay_after_unauthorized(server, &entry, &connection, name, args, session_id)
                .await
                .unwrap_or(Err(error)),
            outcome => outcome,
        };
        match result {
            Ok(result) => Ok(ToolResult {
                is_error: result.is_error,
                structured: result.structured,
                content: result.content,
                server: server.to_string(),
                tool: name.to_string(),
            }),
            Err(error) => Err(ToolCallError {
                server: server.to_string(),
                tool: name.to_string(),
                message: format!(
                    "MCP tool \"{name}\" on server \"{server}\" failed: {}",
                    error.message
                ),
            }
            .into()),
        }
    }

    /// `recovering` (index.ts:350-365): stop + start under the server lock,
    /// then replay the request once against the replacement. The recovery
    /// only runs when the FAILED connection is still the entry's live
    /// client (`recover`'s `entry.client !== connection` early return,
    /// index.ts:344) — the background session-expired watcher may have
    /// already reconnected.
    async fn replay_after_recovery<F, Fut, T>(
        &self,
        server: &str,
        entry: &Arc<ServerEntry>,
        failed: &client::Connection,
        replay: F,
    ) -> Option<Result<T, client::RequestError>>
    where
        F: FnOnce(client::Connection) -> Fut,
        Fut: std::future::Future<Output = Result<T, client::RequestError>>,
    {
        {
            let lock = Self::server_lock(&self.inner, server);
            let _guard = lock.lock().await;
            if entry.client.lock().await.as_ref().is_some_and(|current| current.is_same(failed)) {
                self.stop_server(server, entry).await;
                self.start_server(server, entry).await;
            }
        }
        let replacement = entry.client.lock().await.clone()?;
        Some(replay(replacement).await)
    }

    /// G-359 — runtime OAuth token-expiry recovery (the TS SDK's
    /// `OAuthClientProvider` auto-refreshes on 401; client.ts:149-151):
    /// when an OAuth-credentialed remote server answers 401/403 on a
    /// live connection, refresh the stored grant ONCE, reconnect (the
    /// bearer rides headers baked into the transport's reqwest client at
    /// connect time — the token swap is a stop+start under the server
    /// lock, which also re-runs [`Self::connect_bearer`] against the
    /// rotated credential), and replay the call exactly once. No loop: a
    /// second 401 surfaces as the plain unauthorized error.
    ///
    /// A server without an integration credential, a credential without
    /// a refresh token, or a failed refresh settles the server at
    /// `needs_auth` (G-110's face — the UI re-auth path engages) and the
    /// original unauthorized error surfaces to the caller. A REJECTED
    /// grant additionally drops the dead credential row (G-110③).
    async fn replay_after_unauthorized(
        &self,
        server: &str,
        entry: &Arc<ServerEntry>,
        failed: &client::Connection,
        name: &str,
        args: Option<serde_json::Map<String, serde_json::Value>>,
        session_id: Option<&str>,
    ) -> Option<Result<client::CallToolResult, client::RequestError>> {
        let integration_id = entry.integration_id.clone()?;
        let ServerConfig::Remote(remote) = &entry.config else {
            return None;
        };
        let stored = self.inner.credentials.oauth_credential(&integration_id).await?;
        if stored.refresh.is_empty() {
            self.settle_needs_auth(
                server,
                entry,
                "no refresh token for the expired access token".to_string(),
            )
            .await;
            return None;
        }
        match super::oauth::refresh(&self.inner.http, remote, &stored).await {
            Ok(tokens) => {
                if let Err(error) = self
                    .inner
                    .credentials
                    .save_oauth_tokens(&integration_id, &stored.id, &tokens)
                    .await
                {
                    eprintln!(
                        "opencode mcp: token rotation for \"{}\" could not be stored: {error}",
                        entry.integration_label()
                    );
                }
            }
            Err(error) => {
                eprintln!(
                    "opencode mcp: runtime credential refresh for \"{}\" failed: {error}",
                    entry.integration_label()
                );
                // G-110③: a REJECTED grant invalidates the credential;
                // discovery/network failures do not.
                if super::oauth::is_token_rejected(&error) {
                    if let Err(remove_error) =
                        self.inner.credentials.remove_credential(&stored.id).await
                    {
                        eprintln!(
                            "opencode mcp: could not remove the dead credential for \"{}\": {remove_error}",
                            entry.integration_label()
                        );
                    }
                }
                self.settle_needs_auth(server, entry, error).await;
                return None;
            }
        }
        // The reconnect mirrors `replay_after_recovery`: only when the
        // FAILED connection is still the entry's live client.
        {
            let lock = Self::server_lock(&self.inner, server);
            let _guard = lock.lock().await;
            if entry.client.lock().await.as_ref().is_some_and(|current| current.is_same(failed)) {
                self.stop_server(server, entry).await;
                self.start_server(server, entry).await;
            }
        }
        let replacement = entry.client.lock().await.clone()?;
        Some(replacement.call_tool(name, args.as_ref(), session_id).await)
    }

    /// G-359: settles one server at `needs_auth` with the refresh
    /// failure's text so the re-auth UI path engages.
    async fn settle_needs_auth(&self, server: &str, entry: &Arc<ServerEntry>, error: String) {
        entry.set_status(Status::NeedsAuth { error });
        self.publish("mcp.status.changed", server).await;
    }

    /// `Mcp.instructions` — connected servers with initialization
    /// instructions, sorted by server.
    pub async fn instructions(&self) -> Vec<ServerInstructions> {
        let entries: Vec<(String, Arc<ServerEntry>)> = self
            .inner
            .entries
            .lock()
            .iter()
            .map(|(server, entry)| (server.clone(), Arc::clone(entry)))
            .collect();
        let mut out = Vec::new();
        for (server, entry) in entries {
            let connection = entry.client.lock().await;
            if let Some(instructions) =
                connection.as_ref().and_then(client::Connection::instructions)
            {
                out.push(ServerInstructions { server, instructions });
            }
        }
        out.sort_by(|a, b| oc_util::localecmp::locale_cmp(&a.server, &b.server));
        out
    }

    /// `Mcp.prompts` — sorted by (server, name).
    #[must_use]
    pub fn prompts(&self) -> Vec<Prompt> {
        let mut prompts: Vec<Prompt> = self
            .inner
            .entries
            .lock()
            .values()
            .flat_map(|entry| entry.prompts.lock().clone().unwrap_or_default())
            .collect();
        prompts.sort_by(|a, b| {
            oc_util::localecmp::locale_cmp(&a.server, &b.server)
                .then_with(|| oc_util::localecmp::locale_cmp(&a.name, &b.name))
        });
        prompts
    }

    /// `Mcp.prompt(name, args)` — `Ok(None)` when unavailable.
    pub async fn prompt(
        &self,
        server: &str,
        name: &str,
        args: Option<BTreeMap<String, String>>,
    ) -> Result<Option<serde_json::Value>, McpError> {
        let entry = self.require_entry(server)?;
        entry.await_startup().await;
        let connection = entry.client.lock().await.clone();
        let Some(connection) = connection else { return Ok(None) };
        let result = connection.prompt(name, args.clone()).await;
        let result = match result {
            Err(error) if error.is_session_expired() => self
                .replay_after_recovery(server, &entry, &connection, |connection| async move {
                    connection.prompt(name, args.clone()).await
                })
                .await
                .unwrap_or(Err(error)),
            outcome => outcome,
        };
        match result {
            Ok(result) => Ok(Some(serde_json::to_value(&result).unwrap_or_default())),
            Err(_) => Ok(None),
        }
    }

    /// `Mcp.resourceCatalog` — every connected server merged; per-server
    /// failures collapse to empty catalogs.
    pub async fn resource_catalog(&self) -> ResourceCatalog {
        let mut catalogs = Vec::new();
        let entries: Vec<(String, Arc<ServerEntry>)> = self
            .inner
            .entries
            .lock()
            .iter()
            .map(|(name, entry)| (name.clone(), Arc::clone(entry)))
            .collect();
        for (name, entry) in entries {
            if entry.client.lock().await.is_none() {
                catalogs.push(empty_catalog());
                continue;
            }
            catalogs.push(self.load_catalog(&name, &entry).await.unwrap_or_else(empty_catalog));
        }
        merge_catalogs(&catalogs)
    }

    /// `Mcp.resources({server})`.
    pub async fn resources(&self, server: &str) -> Result<ResourceCatalog, McpError> {
        let entry = self.require_entry(server)?;
        entry.await_startup().await;
        if entry.client.lock().await.is_none() {
            return Ok(empty_catalog());
        }
        Ok(self.load_catalog(server, &entry).await.unwrap_or_else(empty_catalog))
    }

    /// `loadCatalog` (index.ts:367-400) with one expiry recovery + replay.
    async fn load_catalog(&self, name: &str, entry: &Arc<ServerEntry>) -> Option<ResourceCatalog> {
        let connection = entry.client.lock().await.clone()?;
        let catalog = catalog_once(&connection, name).await;
        match catalog {
            Ok(catalog) => Some(catalog),
            Err(error) if error.is_session_expired() => self
                .replay_after_recovery(name, entry, &connection, |connection| async move {
                    catalog_once(&connection, name).await
                })
                .await
                .and_then(Result::ok),
            Err(_) => None,
        }
    }

    /// `Mcp.readResource` — `Ok(None)` when the server is not connected or
    /// does not expose resources.
    pub async fn read_resource(
        &self,
        server: &str,
        uri: &str,
    ) -> Result<Option<ResourceContent>, McpError> {
        let entry = self.require_entry(server)?;
        entry.await_startup().await;
        let connection = entry.client.lock().await.clone();
        let Some(connection) = connection else { return Ok(None) };
        let read = connection.read_resource(uri).await;
        let read = match read {
            Err(error) if error.is_session_expired() => self
                .replay_after_recovery(server, &entry, &connection, |connection| async move {
                    connection.read_resource(uri).await
                })
                .await
                .unwrap_or(Err(error)),
            outcome => outcome,
        };
        let Ok(Some(result)) = read else { return Ok(None) };
        Ok(Some(ResourceContent {
            server: server.to_string(),
            uri: uri.to_string(),
            contents: result
                .contents
                .iter()
                .filter_map(|part| match part {
                    rmcp::model::ResourceContents::TextResourceContents {
                        uri,
                        mime_type,
                        text,
                        ..
                    } => Some(ResourceContentPart::Text {
                        uri: uri.clone(),
                        text: text.clone(),
                        mime_type: mime_type.clone(),
                    }),
                    rmcp::model::ResourceContents::BlobResourceContents {
                        uri,
                        mime_type,
                        blob,
                        ..
                    } => Some(ResourceContentPart::Blob {
                        uri: uri.clone(),
                        blob: blob.clone(),
                        mime_type: mime_type.clone(),
                    }),
                    // Future resource-contents kinds have no v2 mapping.
                    _ => None,
                })
                .collect(),
        }))
    }

    /// Shutdown every connection (location teardown).
    pub async fn shutdown(&self) {
        let entries: Vec<(String, Arc<ServerEntry>)> = self
            .inner
            .entries
            .lock()
            .iter()
            .map(|(name, entry)| (name.clone(), Arc::clone(entry)))
            .collect();
        for (name, entry) in entries {
            self.dispose_server(&name, &entry).await;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchKind {
    Tools,
    Prompts,
}

/// One `loadCatalog` attempt.
async fn catalog_once(
    connection: &client::Connection,
    name: &str,
) -> Result<ResourceCatalog, client::RequestError> {
    let resources = connection.resources().await?;
    // Some servers declare resources without implementing template listing.
    let templates = connection.resource_templates().await.unwrap_or_default();
    Ok(ResourceCatalog {
        resources: resources
            .iter()
            .map(|resource| Resource {
                server: name.to_string(),
                name: resource.name.clone(),
                uri: resource.uri.clone(),
                description: resource.description.as_ref().map(ToString::to_string),
                mime_type: resource.mime_type.as_ref().map(ToString::to_string),
            })
            .collect(),
        templates: templates
            .iter()
            .map(|template| ResourceTemplate {
                server: name.to_string(),
                name: template.name.clone(),
                uri_template: template.uri_template.clone(),
                description: template.description.as_ref().map(ToString::to_string),
                mime_type: template.mime_type.as_ref().map(ToString::to_string),
            })
            .collect(),
    })
}

/// An empty catalog (`ResourceCatalog.make({resources: [], templates: []})`).
#[must_use]
pub fn empty_catalog() -> ResourceCatalog {
    ResourceCatalog { resources: Vec::new(), templates: Vec::new() }
}

/// Merge catalogs sorted by (server, name, uri) (index.ts:756-772).
#[must_use]
pub fn merge_catalogs(catalogs: &[ResourceCatalog]) -> ResourceCatalog {
    let mut resources: Vec<Resource> =
        catalogs.iter().flat_map(|catalog| catalog.resources.clone()).collect();
    let mut templates: Vec<ResourceTemplate> =
        catalogs.iter().flat_map(|catalog| catalog.templates.clone()).collect();
    resources.sort_by(|a, b| {
        oc_util::localecmp::locale_cmp(&a.server, &b.server)
            .then_with(|| oc_util::localecmp::locale_cmp(&a.name, &b.name))
            .then_with(|| oc_util::localecmp::locale_cmp(&a.uri, &b.uri))
    });
    templates.sort_by(|a, b| {
        oc_util::localecmp::locale_cmp(&a.server, &b.server)
            .then_with(|| oc_util::localecmp::locale_cmp(&a.name, &b.name))
            .then_with(|| oc_util::localecmp::locale_cmp(&a.uri_template, &b.uri_template))
    });
    ResourceCatalog { resources, templates }
}

/// `toTool` (index.ts:304-308): tag the SDK tool with its server and the
/// config's Code Mode flag.
fn tag_tool(server: &str, config: &ServerConfig, tool: &rmcp::model::Tool) -> Tool {
    Tool {
        name: tool.name.to_string(),
        description: tool.description.as_ref().map(ToString::to_string),
        input_schema: serde_json::to_value(&*tool.input_schema).unwrap_or_default(),
        output_schema: tool
            .output_schema
            .as_ref()
            .map(|schema| serde_json::to_value(&**schema).unwrap_or_default()),
        server: server.to_string(),
        codemode: match config {
            ServerConfig::Local(local) => local.codemode,
            ServerConfig::Remote(remote) => remote.codemode,
        },
    }
}

/// Whether a config disables the server (`disabled ?? false`).
#[must_use]
pub fn config_disabled(config: &ServerConfig) -> bool {
    match config {
        ServerConfig::Local(local) => local.disabled.unwrap_or(false),
        ServerConfig::Remote(remote) => remote.disabled.unwrap_or(false),
    }
}

fn production_connector() -> Connector {
    Arc::new(|input: ConnectInput<'_>| {
        let server = input.server.to_string();
        let config = input.config.clone();
        let directory = input.directory.to_string();
        let bearer = input.bearer.clone();
        let elicitation = input.elicitation.clone();
        let stdio = input.stdio.clone();
        Box::pin(async move {
            client::connect(ConnectInput {
                server: &server,
                config: &config,
                directory: &directory,
                bearer,
                elicitation,
                stdio,
            })
            .await
        })
    })
}

// ---------------------------------------------------------------------------
// tool-registry bridge (tool/mcp.ts)
// ---------------------------------------------------------------------------

/// `McpTool.namespace` (tool/mcp.ts:16): non-`[a-zA-Z0-9_-]` → `_`.
#[must_use]
pub fn namespace(server: &str) -> String {
    server
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') { c } else { '_' })
        .collect()
}

/// `McpTool.name` (tool/mcp.ts:17): `{serverSanitized}_{toolSanitized}`.
#[must_use]
pub fn tool_name(server: &str, tool: &str) -> String {
    format!("{}_{}", namespace(server), namespace(tool))
}

/// The executor bound to one MCP tool (tool/mcp.ts:49-117): the execute
/// closure FIRST asserts permission (`permission.assert` with
/// action = the full namespaced name, resources and save both `["*"]`,
/// mcp.ts:51-63 — G-242), then runs `mcp.callTool` with
/// structured-output preference (`structuredContent` → JSON-parsed
/// text when the server declares no schema → raw text); media → file
/// parts; tool errors join the text parts or fall back to the generic
/// message.
///
/// Registered through the unified C1 pipeline wrapper
/// (`pipeline_executor_with_output`, G-242): the before/after hooks,
/// `decodeInput`, and `normalizeContent` run around this leaf exactly
/// like ref's `editor.add` registration (mcp.ts:43 → tool.ts:150).
pub struct McpToolExecutor {
    registry: McpRegistry,
    tool: Tool,
    permission: crate::permission::Permission,
}

/// Reserved metadata key carrying the computed tool `output`
/// (`Tool.NormalizedResult.output`) through the Wave-B `ToolRunResult`
/// seam until the C1 pipeline widens it.
pub const OUTPUT_METADATA_KEY: &str = "mcp.output";

#[async_trait::async_trait]
impl ToolExecute for McpToolExecutor {
    async fn execute(&self, input: &serde_json::Value) -> Result<ToolRunResult, ToolRunFailure> {
        self.run(input, None).await
    }

    /// G-360① — the context-aware surface threads the invoking session
    /// into the tool-call metadata (`sessionID: context.sessionID`,
    /// mcp.ts:69/124) so per-session servers (Code Mode included) can
    /// attribute the call.
    async fn execute_with_context(
        &self,
        input: &serde_json::Value,
        context: &crate::location::ToolCallContext,
    ) -> Result<ToolRunResult, ToolRunFailure> {
        self.run(input, Some(context.session_id.0.as_str())).await
    }
}

impl McpToolExecutor {
    /// The leaf body: `mcp.callTool` with the structured-output
    /// preference (`structuredContent` → JSON-parsed text when the
    /// server declares no schema → raw text); media → file parts; tool
    /// errors join the text parts or fall back to the generic message.
    async fn run(
        &self,
        input: &serde_json::Value,
        session_id: Option<&str>,
    ) -> Result<ToolRunResult, ToolRunFailure> {
        let args = input.as_object().cloned();
        let result = self
            .registry
            .call_tool(&self.tool.server, &self.tool.name, args, session_id)
            .await
            .map_err(|error| ToolRunFailure::Error {
                message: match error {
                    McpError::NotFound(error) => {
                        format!("MCP server \"{}\" is not available", error.server)
                    }
                    McpError::ToolCall(error) => error.message,
                },
                metadata: None,
            })?;
        if result.is_error {
            let joined = result
                .content
                .iter()
                .filter_map(|part| match part {
                    client::CallToolContent::Text { text } => Some(text.as_str()),
                    client::CallToolContent::Media { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            let trimmed = joined.trim();
            return Err(ToolRunFailure::Error {
                message: if trimmed.is_empty() {
                    "MCP tool returned an error".to_string()
                } else {
                    trimmed.to_string()
                },
                metadata: None,
            });
        }
        let content: Vec<ToolContent> = result
            .content
            .iter()
            .map(|part| match part {
                client::CallToolContent::Text { text } => ToolContent::Text { text: text.clone() },
                client::CallToolContent::Media { data, mime_type } => ToolContent::File {
                    uri: format!("data:{mime_type};base64,{data}"),
                    mime: mime_type.clone(),
                    name: None,
                },
            })
            .collect();
        let text = content
            .iter()
            .filter_map(|part| match part {
                ToolContent::Text { text } => Some(text.as_str()),
                ToolContent::File { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let output = if let Some(structured) = &result.structured {
            structured.clone()
        } else if text.is_empty() {
            serde_json::Value::Null
        } else if self.tool.output_schema.is_none()
            && (text.starts_with('{') || text.starts_with('['))
        {
            // Agents assume JSON returned as text is already an object, so
            // parse it when the server declares no schema.
            serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text.clone()))
        } else {
            serde_json::Value::String(text)
        };
        let mut metadata = oc_schema::common::Record::default();
        metadata.insert(OUTPUT_METADATA_KEY.into(), output.clone());
        // TOOL-D1: the widened `ToolRunResult.output` seam carries the
        // structured output to the runner (v2 tool/mcp.ts:106-108 returns
        // `{ output: output(), content }`); the metadata stash stays for
        // wire compatibility with the Wave-B shape.
        Ok(ToolRunResult { content, metadata: Some(metadata), output: Some(output) })
    }
}

/// G-242 — the pipeline leaf surface (ref registers the execute closure
/// through `editor.add`, mcp.ts:43, i.e. the unified pipeline wraps this
/// leaf): permission.assert FIRST (mcp.ts:51-63), then the raw callTool
/// logic above. Ask rules park the call exactly like ref; deny surfaces
/// as the permission failure; declines tunnel as interrupts.
impl crate::tool::ToolExecuteWithContext for McpToolExecutor {
    fn execute<'a>(
        &'a self,
        _services: &'a crate::tool::ToolServices,
        input: &'a serde_json::Value,
        context: &'a crate::location::ToolCallContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<crate::tool::ToolOutcome, crate::tool::ToolFailure>,
                > + Send
                + 'a,
        >,
    > {
        use crate::permission::AssertInput;

        Box::pin(async move {
            // mcp.ts:51-63: action = `{server}_{tool}` (the namespaced
            // name), resources and save both `["*"]`, source = the
            // invoking tool call.
            self.permission
                .assert(AssertInput {
                    id: None,
                    session_id: context.session_id.clone(),
                    agent: Some(context.agent.clone()),
                    action: tool_name(&self.tool.server, &self.tool.name),
                    resources: vec!["*".to_string()],
                    save: Some(vec!["*".to_string()]),
                    metadata: None, // ref `metadata: {}` — see G-191's mapping note
                    source: Some(oc_schema::permission::Source::Tool {
                        message_id: context.message_id.clone(),
                        id: context.call_id.clone(),
                    }),
                })
                .await
                .map_err(|error| crate::tool::ToolFailure {
                    // The pipeline's `visible_message` unwraps the cause
                    // (deny → BlockedError text, decline → interrupt);
                    // the blanket matches ref's foreign-error coercion
                    // (`Unable to execute ${name}`, mcp.ts:115).
                    message: format!(
                        "Unable to execute {}",
                        tool_name(&self.tool.server, &self.tool.name)
                    ),
                    cause: Some(crate::tool::permission_cause(&error)),
                })?;
            ToolExecute::execute_with_context(self, input, context)
                .await
                .map(|result| crate::tool::ToolOutcome {
                    content: result.content,
                    metadata: result.metadata,
                    output: result.output,
                })
                .map_err(crate::tool::subagent::run_failure_to_failure)
        })
    }
}

/// The MCP → tool-registry bridge (tool/mcp.ts): one shared registration
/// refresh, driven by `mcp.tools.changed` with a 100ms debounce; the
/// initial registration flushes before the first read (`flush`).
pub struct McpToolBridge {
    /// Resolves once the initial registration settled.
    flush: tokio::sync::watch::Receiver<bool>,
}

impl McpToolBridge {
    /// Installs the bridge against a tool registry. Wholly-denied tools
    /// are dropped by `ToolRegistryState::snapshot` at read time, matching
    /// v2's snapshot filtering. The executors register through the
    /// unified pipeline wrapper (G-242 — ref `editor.add`, mcp.ts:43):
    /// before/after hooks, decodeInput, encodeOutput, normalizeContent.
    #[must_use]
    pub fn install(
        registry: &McpRegistry,
        tools: &Arc<ToolRegistryState>,
        permission: &crate::permission::Permission,
        hooks: &Arc<crate::hooks::HookRegistry>,
        services: &Arc<crate::tool::ToolServices>,
    ) -> McpToolBridge {
        let (flush_tx, flush_rx) = tokio::sync::watch::channel(false);
        // G-360② — the ref serializes the initial registration and the
        // debounced reload through `Semaphore.makeUnsafe(1)` (mcp.ts
        // 132-138): `sync` reads the catalog and swaps the registration
        // table without internal await points, but the initial task and
        // the refresh task run on separate workers, so unserialized syncs
        // could interleave capture/apply and land a STALE table last (a
        // lost-registration overwrite). One bridge-wide lock makes every
        // sync a serialized full reconcile.
        let sync_lock = Arc::new(SyncMutex::new(()));
        // Initial registration: register once after initial discovery; only
        // subsequent updates need the debounced reload.
        let owned = Arc::new(SyncMutex::new(std::collections::BTreeSet::new()));
        let initial = {
            let registry = registry.clone();
            let tools = Arc::clone(tools);
            let permission = permission.clone();
            let hooks = Arc::clone(hooks);
            let services = Arc::clone(services);
            let owned = Arc::clone(&owned);
            let sync_lock = Arc::clone(&sync_lock);
            async move {
                Self::sync(&registry, &tools, &permission, &hooks, &services, &owned, &sync_lock);
                let _ = flush_tx.send(true);
            }
        };
        tokio::spawn(initial);
        // Debounced updates: eager bus subscription, sliding-1 signal, the
        // refresh settles bursts before reloading the whole catalog.
        let (signal_tx, mut signal_rx) = tokio::sync::mpsc::channel::<()>(1);
        let mut subscription = registry.inner.bus.subscribe_type("mcp.tools.changed");
        {
            tokio::spawn(async move {
                use futures::StreamExt;
                while subscription.next().await.is_some() {
                    let _ = signal_tx.try_send(());
                }
            });
        }
        let refresh = {
            let registry = registry.clone();
            let tools = Arc::clone(tools);
            let permission = permission.clone();
            let hooks = Arc::clone(hooks);
            let services = Arc::clone(services);
            let owned = Arc::clone(&owned);
            let sync_lock = Arc::clone(&sync_lock);
            async move {
                while signal_rx.recv().await.is_some() {
                    // Sliding debounce: collapse bursts within 100ms.
                    let deadline = tokio::time::sleep(TOOL_REFRESH_DEBOUNCE);
                    tokio::pin!(deadline);
                    loop {
                        tokio::select! {
                            () = &mut deadline => break,
                            _ = signal_rx.recv() => {},
                        }
                    }
                    Self::sync(
                        &registry,
                        &tools,
                        &permission,
                        &hooks,
                        &services,
                        &owned,
                        &sync_lock,
                    );
                }
            }
        };
        tokio::spawn(refresh);
        McpToolBridge { flush: flush_rx }
    }

    #[allow(clippy::too_many_arguments)]
    fn sync(
        registry: &McpRegistry,
        tools: &ToolRegistryState,
        permission: &crate::permission::Permission,
        hooks: &Arc<crate::hooks::HookRegistry>,
        services: &Arc<crate::tool::ToolServices>,
        owned: &SyncMutex<std::collections::BTreeSet<String>>,
        sync_lock: &SyncMutex<()>,
    ) {
        // G-360② — serialize capture+apply across the initial and refresh
        // tasks (the ref's `Semaphore.makeUnsafe(1)`): without the guard a
        // sync captured before a catalog change could apply after a newer
        // sync and overwrite the table with stale registrations.
        let _guard = sync_lock.lock();
        let discovered = registry.tools();
        // v2 registration shape (tool/mcp.ts:40-45): bare `tool.name` +
        // `options.namespace = server` — the registry derives the
        // effective `server_tool` name and the `server.tool` Code Mode
        // path. The `{server}_{tool}` form is the PERMISSION ACTION
        // (mcp.ts:17), asserted by the executor's leaf (G-242), and —
        // G-291 — the EFFECTIVE registry key: both the registration
        // table and the executor table key by it, so two servers
        // exposing the same bare tool name coexist (tool.ts:188).
        //
        // G-360③ — the ref routes every registration through
        // `editor.add`'s `registrationError` gate (tool.ts:177-183):
        // violators are logged (`Skipping invalid tool registration`,
        // tool.ts:203-211) and SKIPPED, never registered. The bridge
        // applies the same rules here (name/namespace charset + length,
        // the Code Mode `execute` reservation) before touching the
        // tables.
        let mut current: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut valid: Vec<&Tool> = Vec::new();
        for tool in &discovered {
            let action = tool_name(&tool.server, &tool.name);
            let server_namespace = namespace(&tool.server);
            if let Some(error) = crate::tool::registry::namespaced_registration_error(
                Some(&server_namespace),
                &tool.name,
                tool.codemode_enabled(),
            ) {
                eprintln!(
                    "opencode mcp: skipping invalid MCP tool registration \"{action}\" on server \"{}\": {error}",
                    tool.server
                );
                continue;
            }
            current.insert(action);
            valid.push(tool);
        }
        // Remove bridge-owned registrations that left the catalog; other
        // registrations are never touched.
        for stale in owned.lock().difference(&current) {
            tools.remove(stale);
            tools.remove_executor(stale);
        }
        for tool in valid {
            let name = tool.name.clone();
            let action = tool_name(&tool.server, &tool.name);
            let executor = McpToolExecutor {
                registry: registry.clone(),
                tool: tool.clone(),
                permission: permission.clone(),
            };
            let mut registration = ToolRegistration::new(
                name.clone(),
                tool.description.clone().unwrap_or_default(),
                tool.input_schema.clone(),
            );
            registration.namespace = Some(namespace(&tool.server));
            registration.permission = Some(action.clone());
            registration.codemode = tool.codemode_enabled();
            // runtime.ts:24-25 — the output rides the model-facing
            // definition; ref's MCP face ALWAYS declares (mcp.ts:48
            // `output: tool.outputSchema ?? {}`).
            registration.output = Some(
                tool.output_schema.clone().unwrap_or_else(|| serde_json::json!({})),
            );
            tools.register(registration);
            // G-242 — the pipeline wrap (`editor.add` → tool.ts:150):
            // hooks + decodeInput + normalizeContent around the leaf's
            // permission assert + callTool. G-230's MCP face mirrors
            // mcp.ts:48 `output: tool.outputSchema ?? {}`: the tool is
            // ALWAYS declared — a server-undeclared tool validates
            // against the accept-all `{}` (the executor always returns
            // an output, mcp.ts:100-110 `output()`). G-291: the
            // executor is keyed by the EFFECTIVE `server_tool` name.
            tools.register_executor(
                action.clone(),
                crate::tool::registry::pipeline_executor_with_output(
                    &action,
                    tool.input_schema.clone(),
                    Some(
                        tool.output_schema.clone().unwrap_or_else(|| serde_json::json!({})),
                    ),
                    Arc::new(executor),
                    services,
                    hooks,
                ),
            );
            owned.lock().insert(action);
        }
    }

    /// Wait for the initial MCP tool registration to settle.
    pub async fn flush(&self) {
        if *self.flush.borrow() {
            return;
        }
        let mut flush = self.flush.clone();
        flush.changed().await.expect("flush sender lives for the bridge lifetime");
    }
}

// ---------------------------------------------------------------------------
// Code Mode defaults (plugin/mcp-codemode-defaults.ts) — the typed side
// ---------------------------------------------------------------------------

/// The `opencode.mcp.codemode.defaults` table over the typed config
/// (the engine's config source applies it; the oc-plugin-host seed
/// path carries the JSON twin): servers that ARE `CodeMode` themselves
/// (executor.sh URLs, local `executor mcp`) get `OpenCode`'s Code Mode
/// turned `OFF`; `PostHog` gets the `tools` header unless configured. A
/// server with `codemode` set is left exactly as configured.
pub fn apply_codemode_defaults(
    servers: &mut std::collections::BTreeMap<String, oc_schema::mcp::ServerConfig>,
) -> usize {
    let mut touched = 0;
    for config in servers.values_mut() {
        match config {
            oc_schema::mcp::ServerConfig::Local(local) => {
                if local.codemode.is_some() {
                    continue;
                }
                if local.command.first().map(String::as_str) == Some("executor")
                    && local.command.get(1).map(String::as_str) == Some("mcp")
                {
                    local.codemode = Some(false);
                    touched += 1;
                }
            }
            oc_schema::mcp::ServerConfig::Remote(remote) => {
                if remote.codemode.is_some() {
                    continue;
                }
                let endpoint = url_endpoint(&remote.url);
                if endpoint.as_deref().is_some_and(is_executor_endpoint) {
                    remote.codemode = Some(false);
                    touched += 1;
                    continue;
                }
                // PostHog: the header unless `?mode=` or any casing of it
                // is already present.
                let Some((hostname, has_mode_param)) = url_parts(&remote.url) else {
                    continue;
                };
                let has_header = remote.headers.as_ref().is_some_and(|headers| {
                    headers.keys().any(|key| key.eq_ignore_ascii_case("x-posthog-mcp-mode"))
                });
                if is_posthog_host(&hostname) && !has_mode_param && !has_header {
                    remote
                        .headers
                        .get_or_insert_with(BTreeMap::new)
                        .insert("x-posthog-mcp-mode".to_owned(), "tools".to_owned());
                    touched += 1;
                }
            }
        }
    }
    touched
}

/// `${origin}${pathname without trailing slashes}` when the URL parses.
fn url_endpoint(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let after = &rest[end..];
    let path_end = after.find(['?', '#']).unwrap_or(after.len());
    let pathname = &after[..path_end];
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let default_port = if scheme == "https" { ":443" } else { ":80" };
    let host = host.strip_suffix(default_port).unwrap_or(host);
    Some(format!("{scheme}://{host}{}", pathname.trim_end_matches('/')))
}

/// `/^https:\/\/executor\.sh\/[^/]+\/mcp$/`.
fn is_executor_endpoint(endpoint: &str) -> bool {
    let Some(rest) = endpoint.strip_prefix("https://executor.sh/") else {
        return false;
    };
    let Some(segment) = rest.strip_suffix("/mcp") else {
        return false;
    };
    !segment.is_empty() && !segment.contains('/')
}

/// `(hostname, has ?mode= param)`.
fn url_parts(url: &str) -> Option<(String, bool)> {
    let (scheme, rest) = url.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..end].rsplit('@').next().unwrap_or(&rest[..end]).to_string();
    let query = rest.find('?').map(|at| {
        let tail = &rest[at + 1..];
        tail.split('#').next().unwrap_or(tail)
    });
    let has_mode = query
        .is_some_and(|query| query.split('&').any(|pair| pair.split('=').next() == Some("mode")));
    let default_port = if scheme == "https" { ":443" } else { ":80" };
    let host = host.strip_suffix(default_port).unwrap_or(&host).to_string();
    Some((host, has_mode))
}

/// `/^mcp(?:-eu|\.us|\.eu)?\.posthog\.com$/`.
fn is_posthog_host(hostname: &str) -> bool {
    let Some(prefix) = hostname.strip_suffix(".posthog.com") else {
        return false;
    };
    matches!(prefix, "mcp" | "mcp-eu" | "mcp.us" | "mcp.eu")
}

#[cfg(test)]
mod codemode_defaults_tests {
    use super::*;
    use oc_schema::mcp::{LocalConfig, RemoteConfig, ServerConfig};

    fn remote(url: &str) -> ServerConfig {
        ServerConfig::Remote(Box::new(RemoteConfig {
            url: url.to_owned(),
            headers: None,
            oauth: None,
            disabled: None,
            codemode: None,
            timeout: None,
            protocol: None,
        }))
    }

    fn remote_codemode(config: &ServerConfig) -> Option<bool> {
        match config {
            ServerConfig::Remote(remote) => remote.codemode,
            ServerConfig::Local(_) => panic!("remote"),
        }
    }

    fn remote_headers(config: &ServerConfig) -> Option<&BTreeMap<String, String>> {
        match config {
            ServerConfig::Remote(remote) => remote.headers.as_ref(),
            ServerConfig::Local(_) => panic!("remote"),
        }
    }

    #[test]
    fn executor_and_posthog_defaults_apply() {
        let mut servers = BTreeMap::from([
            (
                "executor_remote".to_owned(),
                remote("https://executor.sh/example/mcp?source=opencode"),
            ),
            (
                "executor_local".to_owned(),
                ServerConfig::Local(Box::new(LocalConfig {
                    command: vec!["executor".into(), "mcp".into()],
                    cwd: None,
                    environment: None,
                    disabled: None,
                    codemode: None,
                    timeout: None,
                    protocol: None,
                })),
            ),
            ("cloudflare".to_owned(), remote("https://mcp.cloudflare.com/mcp/")),
            ("posthog".to_owned(), remote("https://mcp-eu.posthog.com/mcp")),
            (
                "explicit".to_owned(),
                ServerConfig::Remote(Box::new(RemoteConfig {
                    url: "https://executor.sh/example/mcp".to_owned(),
                    headers: None,
                    oauth: None,
                    disabled: None,
                    codemode: Some(true),
                    timeout: None,
                    protocol: None,
                })),
            ),
        ]);
        assert_eq!(apply_codemode_defaults(&mut servers), 3);
        assert_eq!(
            remote_codemode(&servers["executor_remote"]),
            Some(false),
            "executor.sh remote: our Code Mode off"
        );
        assert!(
            matches!(&servers["executor_local"], ServerConfig::Local(local) if local.codemode == Some(false)),
            "executor mcp local: our Code Mode off"
        );
        assert_eq!(remote_codemode(&servers["cloudflare"]), None, "unrelated stays unset");
        assert_eq!(
            remote_codemode(&servers["posthog"]),
            None,
            "PostHog keeps OpenCode Code Mode ON"
        );
        assert_eq!(
            remote_headers(&servers["posthog"])
                .and_then(|headers| headers.get("x-posthog-mcp-mode"))
                .map(String::as_str),
            Some("tools"),
            "PostHog's own Code Mode is turned off via the header"
        );
        assert_eq!(remote_codemode(&servers["explicit"]), Some(true), "configured values stay");
    }

    #[test]
    fn posthog_mode_param_and_header_win() {
        let mut with_mode =
            BTreeMap::from([("ph".to_owned(), remote("https://mcp.posthog.com/mcp?mode=cli"))]);
        assert_eq!(apply_codemode_defaults(&mut with_mode), 0);
        assert!(remote_headers(&with_mode["ph"]).is_none(), "?mode= is respected");

        let mut with_header = BTreeMap::from([(
            "ph".to_owned(),
            ServerConfig::Remote(Box::new(RemoteConfig {
                url: "https://mcp.posthog.com/mcp".to_owned(),
                headers: Some(BTreeMap::from([(
                    "X-PostHog-MCP-Mode".to_owned(),
                    "cli".to_owned(),
                )])),
                oauth: None,
                disabled: None,
                codemode: None,
                timeout: None,
                protocol: None,
            })),
        )]);
        assert_eq!(apply_codemode_defaults(&mut with_header), 0);
        assert_eq!(
            remote_headers(&with_header["ph"]).map(BTreeMap::len),
            Some(1),
            "the existing header is untouched"
        );
    }
}
