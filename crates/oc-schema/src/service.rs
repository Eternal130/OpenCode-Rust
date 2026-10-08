//! `service.json` shapes from the CLI (`packages/cli/src/services/service-config.ts`)
//! and the client service registration file
//! (`packages/client/src/effect/service.ts`), plus the TUI theme config
//! (`packages/tui/src/config/index.tsx`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The CLI service configuration file (`service.json` in the state
/// directory): how the background server is launched and reached.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ServiceConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cors: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
}

/// The local service registration file written by the server at startup:
/// url, pid, version, and the private password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ServiceRegistration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub url: String,
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

/// TUI theme config (`tui.json` → `theme`): `{name?, mode?}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ThemeConfig {
    /// Theme name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Color mode; `system` follows the terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ThemeMode>,
}

/// `ThemeConfig.mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    System,
    Dark,
    Light,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_config_wire() {
        let config: ServiceConfig = serde_json::from_value(serde_json::json!({
            "port": 49374,
            "hostname": "127.0.0.1",
            "password": "secret",
            "cors": ["http://localhost:3000"]
        }))
        .unwrap();
        assert_eq!(config.port, Some(49374));
        assert_eq!(serde_json::to_value(&config).unwrap()["port"], 49374);
    }

    #[test]
    fn registration_wire() {
        let registration: ServiceRegistration = serde_json::from_value(serde_json::json!({
            "url": "http://127.0.0.1:49374",
            "pid": 1234,
            "version": "2.0.24"
        }))
        .unwrap();
        assert_eq!(registration.pid, 1234);
        assert!(registration.password.is_none());
    }

    #[test]
    fn theme_wire() {
        let theme: ThemeConfig = serde_json::from_value(serde_json::json!({
            "name": "opencode",
            "mode": "system"
        }))
        .unwrap();
        assert_eq!(theme.mode, Some(ThemeMode::System));
    }
}
