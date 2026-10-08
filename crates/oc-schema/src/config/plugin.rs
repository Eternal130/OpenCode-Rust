//! `config/plugin.ts` — `Config.Plugin` entries.
//!
//! v2 semantics (parsed in `core/src/config/plugin/source.ts`): a plain
//! string is an add directive (`"pkg"`, `"pkg@1.2.3"`, `"github:…"`,
//! `"./rel"`, `"file://…"`); a string starting with `-` is a remove directive
//! (`"-id"`, `"-*"`); an object is `{package, options?}`. Use
//! [`Plugin::operation`] to classify an entry.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `Config.Plugin.Entry` — package spec string or explicit object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Plugin {
    /// `"pkg"`, `"pkg@1.2.3"`, `"github:owner/repo"`, `"file://…"`, or a
    /// remove directive like `"-id"` / `"-*"`.
    Spec(String),
    /// Explicit entry with options.
    Entry {
        /// Package name, spec, or path.
        package: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<BTreeMap<String, Value>>,
    },
}

/// `Config.Plugin.Plugins` — ordered directives applied after discovery, so
/// explicit removes can override auto-discovered packages.
pub type Plugins = Vec<Plugin>;

/// A classified plugin operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    /// Add a package (spec string or `{package, options}`).
    Add {
        /// Package spec or resolved path.
        target: String,
        /// Options object (empty for plain strings).
        options: BTreeMap<String, Value>,
    },
    /// Remove a previously added package (`-id`, `-*`).
    Remove {
        /// Package id or glob.
        target: String,
    },
}

impl Plugin {
    /// `ConfigPluginSource.parse` — classifies this entry. Returns an error
    /// for the bare `"-"` directive, matching v2.
    pub fn operation(&self) -> Result<Operation, String> {
        match self {
            Plugin::Entry { package, options } => Ok(Operation::Add {
                target: package.clone(),
                options: options.clone().unwrap_or_default(),
            }),
            Plugin::Spec(spec) => {
                if !spec.starts_with('-') {
                    return Ok(Operation::Add { target: spec.clone(), options: BTreeMap::new() });
                }
                if spec.len() == 1 {
                    return Err("Plugin remove operation requires a target".to_string());
                }
                Ok(Operation::Remove { target: spec[1..].to_string() })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_and_entry_wire() {
        let plugins: Plugins = serde_json::from_value(serde_json::json!([
            "github:opencode/plugins",
            "-legacy",
            { "package": "local-pkg", "options": { "flag": true } }
        ]))
        .unwrap();
        assert_eq!(plugins.len(), 3);
        assert_eq!(
            plugins[0].operation().unwrap(),
            Operation::Add { target: "github:opencode/plugins".into(), options: BTreeMap::new() }
        );
        assert_eq!(plugins[1].operation().unwrap(), Operation::Remove { target: "legacy".into() });
        assert_eq!(
            plugins[2].operation().unwrap(),
            Operation::Add {
                target: "local-pkg".into(),
                options: BTreeMap::from([("flag".to_string(), serde_json::json!(true))])
            }
        );
        assert!(Plugin::Spec("-".into()).operation().is_err());
        let json = serde_json::to_value(&plugins).unwrap();
        assert_eq!(json[2]["package"], "local-pkg");
    }
}
