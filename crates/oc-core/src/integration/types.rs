//! Wire types for the integration host — byte-shaped to v2's
//! `@opencode/schema/integration` + `@opencode/schema/connection` +
//! `@opencode/schema/credential` unions (camelCase fields, `type`-tagged
//! unions, `status`-tagged attempt unions).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Credential values (schema/credential.ts:29-47)
// ---------------------------------------------------------------------------

/// `Credential.OAuth`: `{type:"oauth", methodID, refresh, access, expires, metadata?}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OAuthValue {
    pub method_id: String,
    pub refresh: String,
    pub access: String,
    /// Absolute epoch-millis deadline; `0` = never expires (copilot).
    #[serde(default)]
    pub expires: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, Value>>,
}

/// `Credential.Key`: `{type:"key", key, metadata?, configuration?}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyValue {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, Value>>,
    /// Form answers captured alongside a manually-entered key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<Value>,
}

/// `Credential.Value` (tag union on `type`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CredentialValue {
    #[serde(rename = "oauth")]
    OAuth(OAuthValue),
    #[serde(rename = "key")]
    Key(KeyValue),
}

impl CredentialValue {
    /// How the connection projects (`method` on Connection.CredentialInfo:
    /// `key` | `oauth`).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            CredentialValue::OAuth(_) => "oauth",
            CredentialValue::Key(_) => "key",
        }
    }
}

/// One stored credential row (`Credential.Info`).
#[derive(Debug, Clone)]
pub struct StoredCredential {
    pub id: String,
    pub integration_id: String,
    pub label: String,
    pub value: CredentialValue,
    pub active: bool,
}

// ---------------------------------------------------------------------------
// Connections (schema/connection.ts)
// ---------------------------------------------------------------------------

/// A runtime problem reported for a connection
/// (`Connection.Status` — only `needs_auth` today).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectionStatus {
    pub status: String, // "needs_auth"
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// `Connection.Info` (tag union on `type`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ConnectionInfo {
    /// `{type:"credential", id, label, method, status?}`
    #[serde(rename = "credential")]
    Credential {
        id: String,
        label: String,
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ConnectionStatus>,
    },
    /// `{type:"env", name, status?}`
    #[serde(rename = "env")]
    Env {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ConnectionStatus>,
    },
}

impl ConnectionInfo {
    /// Identity of a connection (v2 `IntegrationConnection.key`):
    /// `credential:{id}` / `env:{name}` — labels and token rotations do
    /// not identify a new connection.
    #[must_use]
    pub fn connection_key(&self) -> String {
        match self {
            ConnectionInfo::Credential { id, .. } => format!("credential:{id}"),
            ConnectionInfo::Env { name, .. } => format!("env:{name}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Methods (schema/integration.ts:14-38)
// ---------------------------------------------------------------------------

/// `Integration.OAuthMethod` form field (Form.Field shape, pruned to the
/// fields the surface carries).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FormField {
    #[serde(rename = "type")]
    pub type_: String,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<FormSelectOption>>,
    /// `when` visibility conditions (`{key, op:"eq", value}`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<Vec<FormCondition>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FormSelectOption {
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FormCondition {
    pub key: String,
    pub op: String,
    pub value: Value,
}

/// Form answer: field key → scalar answer.
pub type FormAnswer = BTreeMap<String, Value>;

/// `Integration.Method` (tag union on `type`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Method {
    #[serde(rename = "oauth")]
    OAuth {
        id: String,
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        form: Option<Vec<FormField>>,
    },
    #[serde(rename = "command")]
    Command { id: String, label: String, command: Vec<String> },
    #[serde(rename = "key")]
    Key {
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        form: Option<Vec<FormField>>,
    },
    #[serde(rename = "env")]
    Env { names: Vec<String> },
}

impl Method {
    #[must_use]
    pub fn method_type(&self) -> &'static str {
        match self {
            Method::OAuth { .. } => "oauth",
            Method::Command { .. } => "command",
            Method::Key { .. } => "key",
            Method::Env { .. } => "env",
        }
    }

    /// v2's dedupe identity (integration.ts:324-331): oauth/command by id,
    /// key/env by type alone.
    #[must_use]
    pub fn same_slot(&self, other: &Method) -> bool {
        match (self, other) {
            // dedupe by id for oauth/command; by type alone for key/env
            // (integration.ts:324-331)
            (Method::OAuth { id: a, .. }, Method::OAuth { id: b, .. })
            | (Method::Command { id: a, .. }, Method::Command { id: b, .. }) => a == b,
            (Method::Key { .. }, Method::Key { .. }) | (Method::Env { .. }, Method::Env { .. }) => {
                true
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Info + attempts (schema/integration.ts:69-131)
// ---------------------------------------------------------------------------

/// `Integration.Info`: `{id, name, metadata?, methods[], connections[]}`.
#[derive(Debug, Clone, Serialize)]
pub struct IntegrationInfo {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, Value>>,
    pub methods: Vec<Method>,
    pub connections: Vec<ConnectionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq)]
pub struct AttemptTime {
    pub created: i64,
    pub expires: i64,
}

/// `Integration.Attempt`: what `oauth.connect` returns.
#[derive(Debug, Clone, Serialize)]
pub struct Attempt {
    #[serde(rename = "attemptID")]
    pub attempt_id: String,
    pub url: String,
    pub instructions: String,
    /// "auto" | "code"
    pub mode: String,
    pub time: AttemptTime,
}

/// `Integration.AttemptStatus` (tag union on `status`).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum AttemptStatus {
    Pending { time: AttemptTime },
    Complete { time: AttemptTime },
    Failed { message: String, time: AttemptTime },
    Expired { time: AttemptTime },
}

/// `Integration.CommandAttempt`: `{attemptID, time}`.
#[derive(Debug, Clone, Serialize)]
pub struct CommandAttempt {
    #[serde(rename = "attemptID")]
    pub attempt_id: String,
    pub time: AttemptTime,
}

/// `Integration.CommandAttemptStatus` (tag union on `status`).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum CommandAttemptStatus {
    Pending {
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        time: AttemptTime,
    },
    Complete {
        time: AttemptTime,
    },
    Failed {
        message: String,
        time: AttemptTime,
    },
    Expired {
        time: AttemptTime,
    },
}

// ---------------------------------------------------------------------------
// Errors (integration.ts:109-128; mapped to HTTP in oc-server)
// ---------------------------------------------------------------------------

/// v2 `Integration.Error` union, typed.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum IntegrationError {
    /// `CodeRequiredError` — a code-mode attempt completed without a code.
    #[error("Authorization code required")]
    CodeRequired { attempt_id: String },
    /// `AuthorizationError` — authorize/callback/refresh failed.
    #[error("{0}")]
    Authorization(String),
    /// `AttemptNotFoundError` — unknown or foreign attempt.
    #[error("OAuth attempt not found")]
    AttemptNotFound,
    /// Rust-side registry misses (mapped like v2's `Effect.die` paths →
    /// 400 `integration_authorization` / 404 method).
    #[error("OAuth method not found: {0}/{1}")]
    MethodNotFound(String, String),
}
