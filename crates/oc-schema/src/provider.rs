//! Port of `provider.ts`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::Record;
use crate::id::{IntegrationId, ProviderId};

/// `Provider.Package` — npm package name backing a provider.
pub type Package = String;

/// `Provider.Activation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Activation {
    Auto,
    Enabled,
    Disabled,
}

/// `Provider.Compaction` — tagged by `type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Compaction {
    /// Summarize the transcript with a model request.
    Summary,
    /// Delegate compaction to the provider's native mechanism.
    Native,
}

/// `Provider.Transport`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Http,
    /// Falls back to HTTP with a warning on routes without a WebSocket channel.
    Websocket,
}

/// Timeout setting: a number of milliseconds, or `false` to disable.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Timeout {
    Millis(crate::common::Num),
    Disabled(bool),
}

impl Timeout {
    /// Whether the timeout is disabled.
    pub fn is_disabled(&self) -> bool {
        matches!(self, Timeout::Disabled(_))
    }
}

/// `Provider.Settings` — known keys plus arbitrary provider options.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<Timeout>,
    #[serde(rename = "headerTimeout", default, skip_serializing_if = "Option::is_none")]
    pub header_timeout: Option<Timeout>,
    #[serde(rename = "chunkTimeout", default, skip_serializing_if = "Option::is_none")]
    pub chunk_timeout: Option<Timeout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<Compaction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<Transport>,
    /// Providers may define arbitrary settings beyond the shared keys.
    #[serde(flatten)]
    pub rest: Record,
}

/// `Provider.Request` — per-request overlay.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Request {
    #[serde(default)]
    pub settings: Settings,
    pub headers: BTreeMap<String, String>,
    pub body: BTreeMap<String, Value>,
}

/// `Provider.Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Info {
    pub id: ProviderId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical: Option<ProviderId>,
    #[serde(rename = "integrationID", default, skip_serializing_if = "Option::is_none")]
    pub integration_id: Option<IntegrationId>,
    pub name: String,
    pub activation: Activation,
    pub package: Package,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Settings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BTreeMap<String, Value>>,
}

impl Info {
    /// `Provider.Info.empty`.
    pub fn empty(id: ProviderId) -> Self {
        Self {
            id: id.clone(),
            canonical: None,
            integration_id: None,
            name: id.0,
            activation: Activation::Auto,
            package: String::new(),
            settings: None,
            headers: None,
            body: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compaction_wire() {
        assert_eq!(
            serde_json::to_value(Compaction::Native).unwrap(),
            serde_json::json!({"type": "native"})
        );
        let back: Compaction =
            serde_json::from_value(serde_json::json!({"type": "summary"})).unwrap();
        assert_eq!(back, Compaction::Summary);
    }

    #[test]
    fn settings_timeout_variants() {
        let settings: Settings =
            serde_json::from_value(serde_json::json!({"timeout": false, "chunkTimeout": 500}))
                .unwrap();
        assert!(settings.timeout.unwrap().is_disabled());
        assert_eq!(settings.chunk_timeout, Some(Timeout::Millis(crate::common::Num(500.0))));
        assert_eq!(
            serde_json::to_value(&settings).unwrap(),
            serde_json::json!({"timeout": false, "chunkTimeout": 500})
        );
    }

    #[test]
    fn request_settings_defaults_empty() {
        let request = Request::default();
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({"settings": {}, "headers": {}, "body": {}})
        );
    }
}
