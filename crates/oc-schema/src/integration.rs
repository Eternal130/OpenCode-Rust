//! Ports of `connection.ts`, `integration.ts`, `credential.ts` — auth
//! plumbing shared by providers and MCP integrations.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::{NonNegativeInt, Record};
use crate::form;
use crate::id::{CredentialId, IntegrationAttemptId, IntegrationId, IntegrationMethodId};

/// `Connection.Status` — runtime problem reported by an integration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConnectionStatus {
    pub status: ConnectionStatusKind,
    pub message: String,
    /// Sign-in URL when the credential itself stays valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// `Connection.Status.status` — only `needs_auth` today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatusKind {
    NeedsAuth,
}

/// `Connection.CredentialInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CredentialInfo {
    pub id: CredentialId,
    pub label: String,
    /// How the credential was obtained.
    pub method: CredentialMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ConnectionStatus>,
}

/// `Connection.CredentialInfo.method`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CredentialMethod {
    Key,
    Oauth,
}

/// `Connection.EnvInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EnvInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ConnectionStatus>,
}

/// `Connection.Info` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ConnectionInfo {
    /// A stored credential.
    Credential(CredentialInfo),
    /// An environment variable.
    Env(EnvInfo),
}

/// `Integration.Method` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Method {
    /// OAuth flow.
    Oauth {
        id: IntegrationMethodId,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<form::Fields>,
    },
    /// CLI command exchange.
    Command { id: IntegrationMethodId, label: String, command: Vec<String> },
    /// API key (optionally via form).
    Key {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<form::Fields>,
    },
    /// Environment variables.
    Env { names: Vec<String> },
}

/// `Integration.Ref`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IntegrationRef {
    pub id: IntegrationId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, Value>>,
}

/// `Integration.Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IntegrationInfo {
    pub id: IntegrationId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, Value>>,
    pub methods: Vec<Method>,
    pub connections: Vec<ConnectionInfo>,
}

/// `Integration.AttemptTime`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AttemptTime {
    pub created: crate::common::Num,
    pub expires: crate::common::Num,
}

/// `Integration.Attempt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Attempt {
    #[serde(rename = "attemptID")]
    pub attempt_id: IntegrationAttemptId,
    pub url: String,
    pub instructions: String,
    pub mode: AttemptMode,
    pub time: AttemptTime,
}

/// `Integration.Attempt.mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AttemptMode {
    Auto,
    Code,
}

/// `Integration.AttemptStatus` — tagged by `status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum AttemptStatus {
    Pending { time: AttemptTime },
    Complete { time: AttemptTime },
    Failed { message: String, time: AttemptTime },
    Expired { time: AttemptTime },
}

/// `Integration.CommandAttempt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CommandAttempt {
    #[serde(rename = "attemptID")]
    pub attempt_id: IntegrationAttemptId,
    pub time: AttemptTime,
}

/// `Integration.CommandAttemptStatus` — tagged by `status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum CommandAttemptStatus {
    Pending {
        #[serde(default, skip_serializing_if = "Option::is_none")]
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

/// `Credential.Value` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CredentialValue {
    /// OAuth grant.
    Oauth {
        #[serde(rename = "methodID")]
        method_id: IntegrationMethodId,
        refresh: String,
        access: String,
        expires: NonNegativeInt,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Record>,
    },
    /// API key.
    Key {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Record>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        configuration: Option<form::Answer>,
    },
}

/// `Credential.Entry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CredentialEntry {
    pub id: CredentialId,
    #[serde(rename = "integrationID")]
    pub integration_id: IntegrationId,
    pub label: String,
    pub active: bool,
    pub value: CredentialValue,
}

/// `Credential.CreateInput`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CredentialCreateInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<CredentialId>,
    #[serde(rename = "integrationID")]
    pub integration_id: IntegrationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub value: CredentialValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activate: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_value_wire() {
        let value = serde_json::json!({
            "type": "key",
            "key": "sk-abc",
            "configuration": {"workspace": "acme"}
        });
        let parsed: CredentialValue = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(parsed, CredentialValue::Key { .. }));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }

    #[test]
    fn method_oauth_wire() {
        let value = serde_json::json!({
            "type": "oauth",
            "id": "oauth-main",
            "label": "Sign in",
            "form": [ {"type": "string", "key": "workspace"} ]
        });
        let parsed: Method = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(parsed, Method::Oauth { .. }));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }

    #[test]
    fn attempt_status_wire() {
        let value = serde_json::json!({
            "status": "failed",
            "message": "denied",
            "time": {"created": 1, "expires": 2}
        });
        let parsed: AttemptStatus = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(parsed, AttemptStatus::Failed { .. }));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }
}
