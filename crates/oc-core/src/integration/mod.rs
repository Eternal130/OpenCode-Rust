//! The integration host (v2 `packages/core/src/integration.ts`): the
//! registry of auth methods, credential/env connections, OAuth + command
//! attempt lifecycle (10-min lifetime, 1-min terminal retention, 30-s
//! scrub), token refresh, and the runtime connection statuses.

pub mod console;
pub mod copilot;
pub mod credential;
pub mod flows;
pub mod providers;
pub mod providers_enterprise;
#[cfg(test)]
mod tests;
pub mod types;
pub mod vertex;
pub mod wellknown_config;

use crate::bus::{registry, Bus, PublishOptions};
use crate::database::Database;
use credential::{CredentialStore, StoreError};
use dashmap::DashMap;
use futures::future::BoxFuture;
use parking_lot::RwLock;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use types::{
    Attempt, AttemptStatus, AttemptTime, CommandAttempt, CommandAttemptStatus, ConnectionInfo,
    ConnectionStatus, CredentialValue, FormAnswer, FormField, IntegrationError, IntegrationInfo,
    KeyValue, Method, OAuthValue, StoredCredential,
};

/// v2 attempt timings (integration.ts:237-239).
const ATTEMPT_LIFETIME_MS: i64 = 10 * 60 * 1000;
const TERMINAL_RETENTION_MS: i64 = 60 * 1000;
const SCRUB_INTERVAL: Duration = Duration::from_secs(30);
/// Refresh when the token expires within 5 minutes (integration.ts:710).
const REFRESH_MARGIN_MS: i64 = 5 * 60 * 1000;

/// What an `authorize()` produced (integration.ts:57-70).
pub struct Authorization {
    pub url: String,
    pub instructions: String,
    pub expires_at: Option<i64>,
    pub mode: AuthMode,
}

pub enum AuthMode {
    Auto(BoxFuture<'static, Result<CredentialValue, String>>),
    Code(CodeCallback),
}

pub type CodeCallback =
    Arc<dyn Fn(String) -> BoxFuture<'static, Result<CredentialValue, String>> + Send + Sync>;
pub type AuthorizeFn =
    Arc<dyn Fn(FormAnswer) -> BoxFuture<'static, Result<Authorization, String>> + Send + Sync>;
pub type RefreshFn =
    Arc<dyn Fn(OAuthValue) -> BoxFuture<'static, Result<CredentialValue, String>> + Send + Sync>;
pub type LabelFn = Arc<dyn Fn(&CredentialValue) -> Option<String> + Send + Sync>;

/// One registered OAuth method implementation (integration.ts:72-78).
pub struct OAuthImplementation {
    pub authorize: AuthorizeFn,
    pub refresh: Option<RefreshFn>,
    pub label: Option<LabelFn>,
}

/// One registry entry.
struct Entry {
    name: String,
    methods: Vec<Method>,
    implementations: BTreeMap<String, OAuthImplementation>,
}

/// A pending OAuth attempt (integration.ts:242-252).
struct PendingOAuth {
    integration_id: String,
    method_id: String,
    label: Option<String>,
    time: AttemptTime,
    authorization: StoredAuthorization,
    completing: bool,
    persisting: bool,
}

/// The authorization halves kept for later completion. Auto callbacks are
/// forked at connect time (futures are one-shot in Rust; v2 Effects are
/// re-runnable values — behavior parity: v2 also sets `completing: true`
/// for auto attempts at insert, integration.ts:584).
enum StoredAuthorization {
    Auto,
    Code(CodeCallback),
}

/// Terminal attempt states (integration.ts:253-259).
struct TerminalOAuth {
    status: &'static str, // complete | failed | expired
    integration_id: String,
    message: Option<String>,
    remove_at: i64,
    time: AttemptTime,
}

enum AttemptEntry {
    Pending(PendingOAuth),
    Terminal(TerminalOAuth),
}

struct PendingCommand {
    integration_id: String,
    message: RwLock<String>,
    time: AttemptTime,
    persisting: RwLock<bool>,
}

struct TerminalCommand {
    status: &'static str,
    integration_id: String,
    message: Option<String>,
    remove_at: i64,
    time: AttemptTime,
}

enum CommandAttemptEntry {
    Pending(PendingCommand),
    Terminal(TerminalCommand),
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("{0}")]
    Store(#[from] StoreError),
}

/// The integration host. Construct via [`IntegrationHost::new`] (engine
/// boot installs the built-in registry); the scrub loop starts on the
/// first connect inside a tokio runtime.
pub struct IntegrationHost {
    store: CredentialStore,
    bus: Arc<Bus>,
    http: flows::Http,
    entries: RwLock<BTreeMap<String, Entry>>,
    attempts: DashMap<String, AttemptEntry>,
    command_attempts: DashMap<String, CommandAttemptEntry>,
    statuses: DashMap<String, ConnectionStatus>,
    scrub_started: std::sync::Once,
    /// The credential-driven provider bindings snapshot (copilot
    /// transport; `copilot.rs`) shared with model resolvers.
    provider_bindings_shared: Arc<copilot::CredentialBindings>,
    /// The console-issued provider snapshot (`console.rs`) shared with
    /// model resolvers.
    console_providers_shared: Arc<console::ConsoleProviders>,
    /// The resolved well-known config documents (`wellknown_config.rs`)
    /// the config Loader reads.
    wellknown_documents_shared: Arc<wellknown_config::WellknownDocuments>,
    /// The shared database handle (kv reads for well-known sources).
    db: Arc<Database>,
}

fn now_ms() -> i64 {
    crate::database::migrations::now_ms()
}

fn attempt_id() -> String {
    use rand::RngCore;
    use std::fmt::Write as _;
    let mut bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut text = String::with_capacity(4 + 24);
    let _ = write!(text, "con_");
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

impl IntegrationHost {
    /// Boots the host with the built-in registry (catalog key/env methods
    /// + the named OAuth providers — see `providers::install`).
    #[must_use]
    pub fn new(
        db: Arc<Database>,
        bus: Arc<Bus>,
        catalog: &crate::provider_catalog::catalog::Catalog,
        useragent: &str,
    ) -> Arc<Self> {
        let host = Self::empty(db, bus, useragent);
        providers::install(&host, catalog);
        Arc::new(host)
    }

    /// Re-runs the catalog key/env method installation over a refreshed
    /// catalog (CRED-A1, v2 `ctx.integration.reload()` on
    /// `models-dev.refreshed` — models-dev.ts:61-66): new providers'
    /// env/key methods register without a re-boot. The registry reads
    /// live, so no notify is needed.
    pub fn reload_catalog(&self, catalog: &crate::provider_catalog::catalog::Catalog) {
        providers::install_catalog_methods(self, catalog);
    }

    #[must_use]
    pub fn empty(db: Arc<Database>, bus: Arc<Bus>, useragent: &str) -> Self {
        #[allow(clippy::needless_pass_by_value)]
        let db = db;
        Self {
            store: CredentialStore::new(db.clone(), bus.clone()),
            bus,
            http: flows::http_client(useragent),
            entries: RwLock::new(BTreeMap::new()),
            attempts: DashMap::new(),
            command_attempts: DashMap::new(),
            statuses: DashMap::new(),
            scrub_started: std::sync::Once::new(),
            provider_bindings_shared: copilot::CredentialBindings::new(),
            console_providers_shared: console::ConsoleProviders::new(),
            wellknown_documents_shared: wellknown_config::WellknownDocuments::new(),
            db,
        }
    }

    /// The shared HTTP client (per-flow clones keep the user-agent).
    #[must_use]
    pub fn http(&self) -> &flows::Http {
        &self.http
    }

    /// The credential store (engine shares it with the HTTP handlers).
    #[must_use]
    pub fn store(&self) -> &CredentialStore {
        &self.store
    }

    /// The shared database handle (well-known source persistence).
    #[must_use]
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    // -- registry -----------------------------------------------------------

    /// Registers/updates an integration definition (editor.update —
    /// integration.ts:299-309).
    pub fn update_integration(&self, id: &str, name: &str) {
        let mut entries = self.entries.write();
        let entry = entries.entry(id.to_owned()).or_insert_with(|| Entry {
            name: id.to_owned(),
            methods: Vec::new(),
            implementations: BTreeMap::new(),
        });
        name.clone_into(&mut entry.name);
    }

    /// Registers a method (editor.method.update — integration.ts:312-340):
    /// replaces the same-slot method, storing the OAuth implementation.
    pub fn update_method(
        &self,
        integration_id: &str,
        method: Method,
        implementation: Option<OAuthImplementation>,
    ) {
        let oauth_id = match &method {
            Method::OAuth { id, .. } => Some(id.clone()),
            _ => None,
        };
        let mut entries = self.entries.write();
        let entry = entries.entry(integration_id.to_owned()).or_insert_with(|| Entry {
            name: integration_id.to_owned(),
            methods: Vec::new(),
            implementations: BTreeMap::new(),
        });
        if let Some(index) = entry.methods.iter().position(|candidate| candidate.same_slot(&method))
        {
            entry.methods[index] = method;
        } else {
            entry.methods.push(method);
        }
        if let (Some(id), Some(implementation)) = (oauth_id, implementation) {
            entry.implementations.insert(id, implementation);
        }
    }

    /// Removes a method (editor.method.remove — integration.ts:341-352).
    pub fn remove_method(&self, integration_id: &str, method: &Method) {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(integration_id) {
            if let Some(index) =
                entry.methods.iter().position(|candidate| candidate.same_slot(method))
            {
                let removed = entry.methods.remove(index);
                if let Method::OAuth { id, .. } = removed {
                    entry.implementations.remove(&id);
                }
            }
        }
    }

    /// The registry notify (integration.ts:355): transforms publish
    /// `integration.updated`.
    pub async fn notify_updated(&self) {
        if let Some(definition) = registry::by_type(registry::INTEGRATION_UPDATED) {
            let _ = self.bus.publish(definition, json!({}), PublishOptions::default()).await;
        }
    }

    fn implementation(
        &self,
        integration_id: &str,
        method_id: &str,
    ) -> Option<(Method, Arc<OAuthImplementation>)> {
        self.entries.read().get(integration_id).and_then(|entry| {
            let method = entry
                .methods
                .iter()
                .find(|method| matches!(method, Method::OAuth { id, .. } if id == method_id))?;
            let implementation = entry.implementations.get(method_id)?;
            Some((
                method.clone(),
                Arc::new(OAuthImplementation {
                    authorize: implementation.authorize.clone(),
                    refresh: implementation.refresh.clone(),
                    label: implementation.label.clone(),
                }),
            ))
        })
    }

    fn env_connections(methods: &[Method]) -> Vec<ConnectionInfo> {
        methods
            .iter()
            .filter_map(|method| match method {
                Method::Env { names } => Some(
                    names
                        .iter()
                        .filter(|name| std::env::var(name).is_ok())
                        .map(|name| ConnectionInfo::Env { name: name.clone(), status: None }),
                ),
                _ => None,
            })
            .flatten()
            .collect()
    }

    fn credential_connections(rows: &[StoredCredential]) -> Vec<ConnectionInfo> {
        rows.iter()
            .rev()
            .map(|stored| ConnectionInfo::Credential {
                id: stored.id.clone(),
                label: stored.label.clone(),
                method: stored.value.kind().to_owned(),
                status: None,
            })
            .collect()
    }

    // -- projections --------------------------------------------------------

    /// `integration.list` (integration.ts:685-690): all integrations with
    /// connections, sorted by name.
    pub async fn list(&self) -> Result<Vec<IntegrationInfo>, HostError> {
        let inventory: Vec<(String, String, Vec<Method>)> = self
            .entries
            .read()
            .iter()
            .map(|(id, entry)| (id.clone(), entry.name.clone(), entry.methods.clone()))
            .collect();
        let mut grouped: BTreeMap<String, Vec<StoredCredential>> = BTreeMap::new();
        for stored in self.store.all().await? {
            grouped.entry(stored.integration_id.clone()).or_default().push(stored);
        }
        let mut infos = Vec::with_capacity(inventory.len());
        for (id, name, methods) in inventory {
            let mut connections =
                grouped.get(&id).map(|rows| Self::credential_connections(rows)).unwrap_or_default();
            connections.extend(Self::env_connections(&methods));
            let connections = self.project_statuses(&id, connections);
            infos.push(IntegrationInfo { id, name, metadata: None, methods, connections });
        }
        infos.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(infos)
    }

    /// `integration.get` (integration.ts:680-684).
    pub async fn get(&self, id: &str) -> Result<Option<IntegrationInfo>, HostError> {
        let Some((name, methods)) =
            self.entries.read().get(id).map(|entry| (entry.name.clone(), entry.methods.clone()))
        else {
            return Ok(None);
        };
        let mut connections = Self::credential_connections(&self.store.list(id).await?);
        connections.extend(Self::env_connections(&methods));
        Ok(Some(IntegrationInfo {
            id: id.to_owned(),
            name,
            metadata: None,
            methods,
            connections: self.project_statuses(id, connections),
        }))
    }

    /// Projects the runtime statuses onto connections (integration.ts:385-395).
    fn project_statuses(
        &self,
        integration_id: &str,
        connections: Vec<ConnectionInfo>,
    ) -> Vec<ConnectionInfo> {
        connections
            .into_iter()
            .map(|connection| {
                let key = format!("{integration_id}:{}", connection.connection_key());
                let status = self.statuses.get(&key).map(|entry| entry.value().clone());
                match connection {
                    ConnectionInfo::Credential { id, label, method, status: existing } => {
                        ConnectionInfo::Credential {
                            id,
                            label,
                            method,
                            status: existing.or(status),
                        }
                    }
                    ConnectionInfo::Env { name, status: existing } => {
                        ConnectionInfo::Env { name, status: existing.or(status) }
                    }
                }
            })
            .collect()
    }

    /// `connection.active` (integration.ts:692-695): the first connection.
    pub async fn connection_active(&self, id: &str) -> Result<Option<ConnectionInfo>, HostError> {
        let methods =
            self.entries.read().get(id).map(|entry| entry.methods.clone()).unwrap_or_default();
        let mut connections = Self::credential_connections(&self.store.list(id).await?);
        connections.extend(Self::env_connections(&methods));
        Ok(connections.into_iter().next())
    }

    /// `connection.resolve` (integration.ts:696-714): env → key from env;
    /// credential → value, refreshing OAuth tokens expiring within 5 min.
    pub async fn connection_resolve(
        &self,
        connection: &ConnectionInfo,
    ) -> Result<Option<CredentialValue>, IntegrationError> {
        match connection {
            ConnectionInfo::Env { name, .. } => Ok(std::env::var(name).ok().map(|key| {
                CredentialValue::Key(KeyValue { key, metadata: None, configuration: None })
            })),
            ConnectionInfo::Credential { id, .. } => {
                let stored = self
                    .store
                    .get(id)
                    .await
                    .map_err(|error| IntegrationError::Authorization(error.to_string()))?;
                let Some(stored) = stored else { return Ok(None) };
                let CredentialValue::OAuth(oauth) = &stored.value else {
                    return Ok(Some(stored.value));
                };
                let Some((_, implementation)) =
                    self.implementation(&stored.integration_id, &oauth.method_id)
                else {
                    return Ok(Some(stored.value));
                };
                let Some(refresh) = implementation.refresh.as_ref() else {
                    return Ok(Some(stored.value));
                };
                if oauth.expires > now_ms() + REFRESH_MARGIN_MS {
                    return Ok(Some(stored.value));
                }
                let refreshed =
                    refresh(oauth.clone()).await.map_err(IntegrationError::Authorization)?;
                let _ = self.store.update(id, None, Some(&refreshed)).await;
                Ok(Some(refreshed))
            }
        }
    }

    /// `connection.key` (integration.ts:715-738): validate the answer
    /// against the key method's form, store the secret.
    pub async fn connect_key(
        &self,
        integration_id: &str,
        key: &str,
        answer: &FormAnswer,
        label: Option<&str>,
    ) -> Result<(), IntegrationError> {
        let method = self.entries.read().get(integration_id).and_then(|entry| {
            entry.methods.iter().find(|method| matches!(method, Method::Key { .. })).cloned()
        });
        let Some(Method::Key { form, .. }) = method else {
            return Err(IntegrationError::Authorization(format!(
                "Key method not found: {integration_id}"
            )));
        };
        if let Some(form) = &form {
            if let Some(invalid) = validate_form(form, answer) {
                return Err(IntegrationError::Authorization(invalid));
            }
        } else if !answer.is_empty() {
            return Err(IntegrationError::Authorization(
                "Key method does not accept a form answer".into(),
            ));
        }
        let value = CredentialValue::Key(KeyValue {
            key: key.to_owned(),
            metadata: None,
            configuration: (!answer.is_empty())
                .then(|| serde_json::to_value(answer).unwrap_or(Value::Null)),
        });
        self.create_credential(integration_id, label, &value)
            .await
            .map_err(|error| IntegrationError::Authorization(error.to_string()))?;
        Ok(())
    }

    /// `connection.status` (integration.ts:744-750): set/clear a runtime
    /// status; notifies `integration.updated`.
    pub async fn connection_status(
        &self,
        integration_id: &str,
        connection: &ConnectionInfo,
        status: Option<ConnectionStatus>,
    ) {
        let key = format!("{integration_id}:{}", connection.connection_key());
        match status {
            Some(status) => {
                self.statuses.insert(key, status);
            }
            None => {
                self.statuses.remove(&key);
            }
        }
        self.notify_updated().await;
    }

    /// The label-deriving credential create (integration.ts:358-367):
    /// `name`, `name 2`, `name 3`, … when no label was given.
    async fn create_credential(
        &self,
        integration_id: &str,
        label: Option<&str>,
        value: &CredentialValue,
    ) -> Result<StoredCredential, StoreError> {
        if let Some(label) = label {
            let (stored, _) = self
                .store
                .create(credential::CreateInput {
                    id: None,
                    integration_id,
                    label: Some(label),
                    value,
                    activate: None,
                })
                .await?;
            return Ok(stored);
        }
        let name = self
            .entries
            .read()
            .get(integration_id)
            .map_or_else(|| integration_id.to_owned(), |entry| entry.name.clone());
        let labels: std::collections::HashSet<String> =
            self.store.list(integration_id).await?.into_iter().map(|stored| stored.label).collect();
        let candidate = (0..=labels.len())
            .map(|index| if index == 0 { name.clone() } else { format!("{name} {}", index + 1) })
            .find(|candidate| !labels.contains(candidate))
            .unwrap_or(name);
        let (stored, _) = self
            .store
            .create(credential::CreateInput {
                id: None,
                integration_id,
                label: Some(&candidate),
                value,
                activate: None,
            })
            .await?;
        Ok(stored)
    }

    // -- oauth attempts ------------------------------------------------------

    /// `oauth.connect` (integration.ts:558-608).
    pub async fn oauth_connect(
        self: &Arc<Self>,
        integration_id: &str,
        method_id: &str,
        answer: &FormAnswer,
        label: Option<&str>,
    ) -> Result<Attempt, IntegrationError> {
        self.start_scrub();
        let Some((method, implementation)) = self.implementation(integration_id, method_id) else {
            return Err(IntegrationError::Authorization(format!(
                "OAuth method not found: {integration_id}/{method_id}"
            )));
        };
        if let Method::OAuth { form: Some(form), .. } = &method {
            if let Some(invalid) = validate_form(form, answer) {
                return Err(IntegrationError::Authorization(invalid));
            }
        }
        let authorization = (implementation.authorize)(answer.clone())
            .await
            .map_err(IntegrationError::Authorization)?;
        let id = attempt_id();
        let created = now_ms();
        let time = AttemptTime {
            created,
            expires: authorization.expires_at.unwrap_or(created + ATTEMPT_LIFETIME_MS),
        };
        let mode =
            matches!(authorization.mode, AuthMode::Auto(_)).then_some("auto").unwrap_or("code");
        let attempt = Attempt {
            attempt_id: id.clone(),
            url: authorization.url,
            instructions: authorization.instructions,
            mode: mode.to_owned(),
            time,
        };
        let mut auto_callback = None;
        let pending = PendingOAuth {
            integration_id: integration_id.to_owned(),
            method_id: method_id.to_owned(),
            label: label.map(String::from),
            time,
            authorization: match authorization.mode {
                AuthMode::Auto(callback) => {
                    auto_callback = Some(callback);
                    StoredAuthorization::Auto
                }
                AuthMode::Code(callback) => StoredAuthorization::Code(callback),
            },
            completing: mode == "auto",
            persisting: false,
        };
        self.attempts.insert(id.clone(), AttemptEntry::Pending(pending));
        if let Some(callback) = auto_callback {
            // Fork the auto callback; settle persists the credential
            // (integration.ts:594-600).
            let host = Arc::clone(self);
            let attempt = id.clone();
            let integration = integration_id.to_owned();
            tokio::spawn(async move {
                let outcome = callback.await.map_err(IntegrationError::Authorization);
                host.settle(&attempt, &integration, outcome).await;
            });
        }
        Ok(attempt)
    }

    /// `oauth.status` (integration.ts:754-761).
    #[allow(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "async surface parity with integration.ts"
    )]
    pub async fn oauth_status(
        &self,
        integration_id: &str,
        attempt_id: &str,
    ) -> Result<AttemptStatus, IntegrationError> {
        let entry = self.attempts.get(attempt_id);
        let Some(entry) = entry else { return Err(IntegrationError::AttemptNotFound) };
        match &entry.value() {
            AttemptEntry::Pending(pending) => {
                if pending.integration_id != integration_id {
                    return Err(IntegrationError::AttemptNotFound);
                }
                Ok(AttemptStatus::Pending { time: pending.time })
            }
            AttemptEntry::Terminal(terminal) => {
                if terminal.integration_id != integration_id {
                    return Err(IntegrationError::AttemptNotFound);
                }
                Ok(match terminal.status {
                    "complete" => AttemptStatus::Complete { time: terminal.time },
                    "expired" => AttemptStatus::Expired { time: terminal.time },
                    _ => AttemptStatus::Failed {
                        message: terminal
                            .message
                            .clone()
                            .unwrap_or_else(|| "Authorization failed".into()),
                        time: terminal.time,
                    },
                })
            }
        }
    }

    /// `oauth.complete` (integration.ts:762-784): drive a code-mode
    /// attempt to its terminal state. Auto attempts were forked at connect
    /// (`completing: true`) and die with "already completing" here.
    pub async fn oauth_complete(
        self: &Arc<Self>,
        integration_id: &str,
        attempt_id: &str,
        code: Option<&str>,
    ) -> Result<(), IntegrationError> {
        let callback = {
            let mut entry =
                self.attempts.get_mut(attempt_id).ok_or(IntegrationError::AttemptNotFound)?;
            let AttemptEntry::Pending(pending) = entry.value_mut() else {
                return Ok(());
            };
            if pending.integration_id != integration_id {
                return Err(IntegrationError::AttemptNotFound);
            }
            if matches!(pending.authorization, StoredAuthorization::Code(_)) && code.is_none() {
                return Err(IntegrationError::CodeRequired { attempt_id: attempt_id.to_owned() });
            }
            if pending.completing {
                // v2 dies here (integration.ts:775-776); mapped to the
                // authorization 400 for a typed surface.
                return Err(IntegrationError::Authorization(format!(
                    "OAuth attempt already completing: {attempt_id}"
                )));
            }
            pending.completing = true;
            match &pending.authorization {
                StoredAuthorization::Code(callback) => {
                    Some(callback(code.unwrap_or_default().to_owned()))
                }
                StoredAuthorization::Auto => None,
            }
        };
        let Some(callback) = callback else {
            return Ok(());
        };
        let outcome = callback.await.map_err(IntegrationError::Authorization);
        self.settle(attempt_id, integration_id, outcome).await;
        match self.oauth_status(integration_id, attempt_id).await {
            Ok(AttemptStatus::Failed { message, .. }) => {
                Err(IntegrationError::Authorization(message))
            }
            _ => Ok(()),
        }
    }

    /// `oauth.cancel` (integration.ts:785-795).
    #[allow(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "async surface parity with integration.ts"
    )]
    pub async fn oauth_cancel(&self, integration_id: &str, attempt_id: &str) {
        if let Some(entry) = self.attempts.get(attempt_id) {
            if let AttemptEntry::Pending(pending) = entry.value() {
                if pending.integration_id == integration_id && !pending.persisting {
                    drop(entry);
                    self.attempts.remove(attempt_id);
                }
            }
        }
    }

    /// The settle step (integration.ts:408-461): persist the credential,
    /// transition to terminal, retain for 1 minute.
    pub async fn settle(
        &self,
        attempt_id: &str,
        integration_id: &str,
        outcome: Result<CredentialValue, IntegrationError>,
    ) {
        let snapshot = {
            let Some(mut entry) = self.attempts.get_mut(attempt_id) else { return };
            let AttemptEntry::Pending(pending) = entry.value_mut() else { return };
            if pending.persisting {
                return;
            }
            if outcome.is_err() {
                let message = match &outcome {
                    Err(IntegrationError::Authorization(message)) => message.clone(),
                    Err(error) => error.to_string(),
                    Ok(_) => unreachable!(),
                };
                let time = pending.time;
                *entry.value_mut() = AttemptEntry::Terminal(TerminalOAuth {
                    status: "failed",
                    integration_id: integration_id.to_owned(),
                    message: Some(message),
                    remove_at: now_ms() + TERMINAL_RETENTION_MS,
                    time,
                });
                return;
            }
            pending.persisting = true;
            (
                pending.integration_id.clone(),
                pending.method_id.clone(),
                pending.label.clone(),
                pending.time,
            )
        };
        let (integration, method_id, label, time) = snapshot;
        let label = match (&outcome, label) {
            (_, Some(label)) => Some(label),
            (Ok(value), None) => self.derive_label(&integration, &method_id, value),
            (Err(_), None) => None,
        };
        let persistence = match &outcome {
            Ok(value) => {
                self.create_credential(&integration, label.as_deref(), value).await.map(|_| ())
            }
            Err(_) => Ok(()),
        };
        if let Some(mut entry) = self.attempts.get_mut(attempt_id) {
            *entry.value_mut() = AttemptEntry::Terminal(TerminalOAuth {
                status: if persistence.is_ok() { "complete" } else { "failed" },
                integration_id: integration.clone(),
                message: persistence.as_ref().err().map(std::string::ToString::to_string),
                remove_at: now_ms() + TERMINAL_RETENTION_MS,
                time,
            });
        }
    }

    /// Implementation-provided label (integration.ts:436-440).
    fn derive_label(
        &self,
        integration_id: &str,
        method_id: &str,
        value: &CredentialValue,
    ) -> Option<String> {
        self.entries
            .read()
            .get(integration_id)
            .and_then(|entry| entry.implementations.get(method_id))
            .and_then(|implementation| implementation.label.as_ref())
            .and_then(|label| label(value))
    }

    // -- command attempts -----------------------------------------------------

    /// `command.connect` (integration.ts:610-674).
    #[allow(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "async surface parity with integration.ts"
    )]
    pub async fn command_connect(
        self: &Arc<Self>,
        integration_id: &str,
        method_id: &str,
        label: Option<&str>,
    ) -> Result<CommandAttempt, IntegrationError> {
        self.start_scrub();
        let method = self.entries.read().get(integration_id).and_then(|entry| {
            entry
                .methods
                .iter()
                .find(|method| matches!(method, Method::Command { id, .. } if id == method_id))
                .cloned()
        });
        let Some(Method::Command { command, .. }) = method else {
            return Err(IntegrationError::MethodNotFound(
                integration_id.to_owned(),
                method_id.to_owned(),
            ));
        };
        if command.is_empty() {
            return Err(IntegrationError::Authorization(format!(
                "Command method not found: {integration_id}/{method_id}"
            )));
        }
        let id = attempt_id();
        let created = now_ms();
        let time = AttemptTime { created, expires: created + ATTEMPT_LIFETIME_MS };
        self.command_attempts.insert(
            id.clone(),
            CommandAttemptEntry::Pending(PendingCommand {
                integration_id: integration_id.to_owned(),
                message: RwLock::new(String::new()),
                time,
                persisting: RwLock::new(false),
            }),
        );
        // Spawn the command (ambient env, stdin ignored); stdout trimmed
        // becomes the key credential (integration.ts:638-671).
        let host = Arc::clone(self);
        let attempt = id.clone();
        let integration = integration_id.to_owned();
        let label_owned = label.map(String::from);
        tokio::spawn(async move {
            let output = tokio::process::Command::new(&command[0])
                .args(&command[1..])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
                .await;
            let output = match output {
                Ok(output) => output,
                Err(error) => {
                    host.settle_command(
                        &attempt,
                        &integration,
                        Err(format!("Failed to spawn authentication command: {error}")),
                    )
                    .await;
                    return;
                }
            };
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                host.settle_command(
                    &attempt,
                    &integration,
                    Err(if stderr.is_empty() {
                        format!(
                            "Authentication command exited {}",
                            output.status.code().unwrap_or(-1)
                        )
                    } else {
                        stderr
                    }),
                )
                .await;
                return;
            }
            let credential = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if credential.is_empty() {
                host.settle_command(
                    &attempt,
                    &integration,
                    Err("Authentication command returned no credential".into()),
                )
                .await;
                return;
            }
            host.settle_command(
                &attempt,
                &integration,
                Ok((label_owned.as_deref().unwrap_or(""), credential)),
            )
            .await;
        });
        Ok(CommandAttempt { attempt_id: id, time })
    }

    /// `command.status` (integration.ts:799-813).
    #[allow(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "async surface parity with integration.ts"
    )]
    pub async fn command_status(
        &self,
        integration_id: &str,
        attempt_id: &str,
    ) -> Result<CommandAttemptStatus, IntegrationError> {
        let entry = self.command_attempts.get(attempt_id);
        let Some(entry) = entry else { return Err(IntegrationError::AttemptNotFound) };
        match entry.value() {
            CommandAttemptEntry::Pending(pending) => {
                if pending.integration_id != integration_id {
                    return Err(IntegrationError::AttemptNotFound);
                }
                let message = pending.message.read().clone();
                Ok(CommandAttemptStatus::Pending {
                    message: (!message.is_empty()).then_some(message),
                    time: pending.time,
                })
            }
            CommandAttemptEntry::Terminal(terminal) => {
                if terminal.integration_id != integration_id {
                    return Err(IntegrationError::AttemptNotFound);
                }
                Ok(match terminal.status {
                    "complete" => CommandAttemptStatus::Complete { time: terminal.time },
                    "expired" => CommandAttemptStatus::Expired { time: terminal.time },
                    _ => CommandAttemptStatus::Failed {
                        message: terminal
                            .message
                            .clone()
                            .unwrap_or_else(|| "Authentication failed".into()),
                        time: terminal.time,
                    },
                })
            }
        }
    }

    /// `command.cancel` (integration.ts:814-824).
    #[allow(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "async surface parity with integration.ts"
    )]
    pub async fn command_cancel(&self, integration_id: &str, attempt_id: &str) {
        if let Some(entry) = self.command_attempts.get(attempt_id) {
            if let CommandAttemptEntry::Pending(pending) = entry.value() {
                if pending.integration_id == integration_id && !*pending.persisting.read() {
                    drop(entry);
                    self.command_attempts.remove(attempt_id);
                }
            }
        }
    }

    /// Command settle (integration.ts:463-507): persist stdout as a key
    /// credential or fail with the message.
    async fn settle_command(
        &self,
        attempt_id: &str,
        integration_id: &str,
        outcome: Result<(&str, String), String>,
    ) {
        let time = {
            let Some(mut entry) = self.command_attempts.get_mut(attempt_id) else { return };
            let CommandAttemptEntry::Pending(pending) = entry.value_mut() else { return };
            if *pending.persisting.read() {
                return;
            }
            if outcome.is_err() {
                let time = pending.time;
                *entry.value_mut() = CommandAttemptEntry::Terminal(TerminalCommand {
                    status: "failed",
                    integration_id: integration_id.to_owned(),
                    message: outcome.clone().err(),
                    remove_at: now_ms() + TERMINAL_RETENTION_MS,
                    time,
                });
                return;
            }
            *pending.persisting.write() = true;
            pending.time
        };
        let persistence = match &outcome {
            Ok((label, credential)) => {
                let value = CredentialValue::Key(KeyValue {
                    key: credential.clone(),
                    metadata: None,
                    configuration: None,
                });
                self.create_credential(integration_id, (label.is_empty()).then_some(*label), &value)
                    .await
                    .map(|_| ())
            }
            Err(_) => Ok(()),
        };
        if let Some(mut entry) = self.command_attempts.get_mut(attempt_id) {
            *entry.value_mut() = CommandAttemptEntry::Terminal(TerminalCommand {
                status: if persistence.is_ok() { "complete" } else { "failed" },
                integration_id: integration_id.to_owned(),
                message: persistence.as_ref().err().map(std::string::ToString::to_string),
                remove_at: now_ms() + TERMINAL_RETENTION_MS,
                time,
            });
        }
    }

    /// Starts the 30-s scrub loop once (integration.ts:555-556).
    fn start_scrub(self: &Arc<Self>) {
        self.scrub_started.call_once(|| {
            let host = Arc::clone(self);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(SCRUB_INTERVAL).await;
                    host.scrub_once();
                }
            });
        });
    }

    /// One scrub pass (integration.ts:509-553): expire overdue pending
    /// attempts, drop overdue terminal ones.
    fn scrub_once(&self) {
        let now = now_ms();
        for entry in &self.attempts {
            let (key, transition) = match entry.value() {
                AttemptEntry::Pending(pending) => {
                    if !pending.persisting && pending.time.expires <= now {
                        (
                            entry.key().clone(),
                            Some(TerminalOAuth {
                                status: "expired",
                                integration_id: pending.integration_id.clone(),
                                message: None,
                                remove_at: now + TERMINAL_RETENTION_MS,
                                time: pending.time,
                            }),
                        )
                    } else {
                        continue;
                    }
                }
                AttemptEntry::Terminal(terminal) => {
                    if terminal.remove_at <= now {
                        (entry.key().clone(), None)
                    } else {
                        continue;
                    }
                }
            };
            drop(entry);
            match transition {
                Some(terminal) => {
                    if let Some(mut entry) = self.attempts.get_mut(&key) {
                        *entry.value_mut() = AttemptEntry::Terminal(terminal);
                    }
                }
                None => {
                    self.attempts.remove(&key);
                }
            }
        }
        for entry in &self.command_attempts {
            let (key, transition) = match entry.value() {
                CommandAttemptEntry::Pending(pending) => {
                    if !*pending.persisting.read() && pending.time.expires <= now {
                        (
                            entry.key().clone(),
                            Some(TerminalCommand {
                                status: "expired",
                                integration_id: pending.integration_id.clone(),
                                message: None,
                                remove_at: now + TERMINAL_RETENTION_MS,
                                time: pending.time,
                            }),
                        )
                    } else {
                        continue;
                    }
                }
                CommandAttemptEntry::Terminal(terminal) => {
                    if terminal.remove_at <= now {
                        (entry.key().clone(), None)
                    } else {
                        continue;
                    }
                }
            };
            drop(entry);
            match transition {
                Some(terminal) => {
                    if let Some(mut entry) = self.command_attempts.get_mut(&key) {
                        *entry.value_mut() = CommandAttemptEntry::Terminal(terminal);
                    }
                }
                None => {
                    self.command_attempts.remove(&key);
                }
            }
        }
    }
}

/// v2's `Form.validateFields`/`validateAnswer` subset: required fields
/// must be answered (respecting `when` conditions and defaults); pattern
/// fields must match.
fn validate_form(form: &[FormField], answer: &FormAnswer) -> Option<String> {
    for field in form {
        if !field_visible(field, answer) {
            continue;
        }
        let provided = answer.get(&field.key);
        match provided {
            None | Some(Value::Null) => {
                if field.default.is_none() && field.required {
                    return Some(format!("Missing required field: {}", field.key));
                }
            }
            Some(value) => {
                if field.required
                    && value.as_str().is_none_or(str::is_empty)
                    && field.default.is_none()
                {
                    return Some(format!("Missing required field: {}", field.key));
                }
                if let (Some(pattern), Some(text)) = (&field.pattern, value.as_str()) {
                    let regex = regex::Regex::new(pattern).ok()?;
                    if !regex.is_match(text) {
                        return Some(format!("Invalid value for field: {}", field.key));
                    }
                }
            }
        }
    }
    None
}

/// `when` visibility conditions (all must match).
fn field_visible(field: &FormField, answer: &FormAnswer) -> bool {
    field.when.as_ref().is_none_or(|conditions| {
        conditions.iter().all(|condition| {
            condition.op == "eq"
                && answer.get(&condition.key).is_some_and(|value| *value == condition.value)
        })
    })
}
