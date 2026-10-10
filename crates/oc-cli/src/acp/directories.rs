//! Additional-directory grants (port of `acp/directories.ts`).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::acp::error::AcpError;

/// `key` (directories.ts:11): the session metadata slot.
pub const KEY: &str = "opencode.acp.additionalDirectories";

/// `parse` (directories.ts:15-22): absolute, glob-free, de-duplicated,
/// and never the root itself.
pub fn parse(cwd: &str, directories: &[String]) -> Result<Vec<String>, AcpError> {
    for directory in directories {
        if !Path::new(directory).is_absolute() || directory.contains(['*', '?']) {
            return Err(AcpError::InvalidAdditionalDirectory { directory: directory.clone() });
        }
    }
    let root = resolve_path(cwd);
    let mut seen: Vec<String> = Vec::new();
    for directory in directories {
        let resolved = resolve_path(directory);
        if resolved != root && !seen.contains(&resolved) {
            seen.push(resolved);
        }
    }
    Ok(seen)
}

/// `grant` (directories.ts:24-27): create-body permissions + metadata.
#[must_use]
pub fn grant(directories: &[String]) -> (Option<Value>, Option<Value>) {
    if directories.is_empty() {
        return (None, None);
    }
    (Some(rules(directories)), Some(json!({ KEY: directories })))
}

/// `list` (directories.ts:29-31).
#[must_use]
pub fn list(metadata: Option<&Value>) -> Vec<String> {
    metadata
        .and_then(|metadata| metadata.get(KEY))
        .and_then(Value::as_array)
        .map(|values| {
            values.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// `activate` (directories.ts:33-60): replace the session's ACP-owned
/// permission rules and metadata. Returns the PATCH body when anything
/// changes.
#[must_use]
pub fn activate(
    session_id: &str,
    permissions: Option<&[Value]>,
    metadata: Option<&Value>,
    directories: &[String],
) -> Option<Value> {
    let previous = list(metadata);
    let owned = rules(&previous);
    let current: Vec<Value> = permissions.map_or_else(Vec::new, <[Value]>::to_vec);
    // ACP rules lead so the session's other rules keep precedence for the
    // same paths (directories.ts:41-45).
    let next: Vec<Value> = rules(directories)
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .chain(
            current
                .iter()
                .filter(|rule| !owned.as_array().is_some_and(|owned| owned.contains(rule)))
                .cloned(),
        )
        .collect::<Vec<_>>();
    let next_metadata = {
        let mut map = serde_json::Map::new();
        if let Some(metadata) = metadata.and_then(Value::as_object) {
            for (name, value) in metadata {
                if name != KEY {
                    map.insert(name.clone(), value.clone());
                }
            }
        }
        if !directories.is_empty() {
            map.insert(KEY.to_owned(), json!(directories));
        }
        Value::Object(map)
    };
    let permissions_changed = next != current;
    let metadata_changed = previous != directories;
    if !permissions_changed && !metadata_changed {
        return None;
    }
    let mut body = serde_json::Map::new();
    if permissions_changed {
        body.insert("permissions".to_owned(), Value::Array(next));
    }
    if metadata_changed {
        body.insert("metadata".to_owned(), next_metadata);
    }
    let _ = session_id;
    Some(Value::Object(body))
}

/// `rules` (directories.ts:63-69): both spellings of a root are granted
/// (tools compare the written path without following symlinks).
#[must_use]
pub fn rules(directories: &[String]) -> Value {
    let mut resources: Vec<String> = Vec::new();
    for directory in directories {
        for spelling in [directory.clone(), resolve_path(directory)] {
            let resource = format!("{spelling}/*");
            if !resources.contains(&resource) {
                resources.push(resource);
            }
        }
    }
    Value::Array(
        resources
            .into_iter()
            .map(|resource| {
                json!({ "action": "external_directory", "resource": resource, "effect": "allow" })
            })
            .collect(),
    )
}

fn resolve_path(path: &str) -> String {
    let path =
        if path.starts_with('/') { PathBuf::from(path) } else { PathBuf::from(".").join(path) };
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            std::path::Component::CurDir => {}
            other => resolved.push(other.as_os_str()),
        }
    }
    resolved.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_or_globbed_directories_are_rejected() {
        assert!(parse("/w", &["rel".to_owned()]).is_err());
        assert!(parse("/w", &["/w/x*".to_owned()]).is_err());
        assert!(parse("/w", &["/w/x?".to_owned()]).is_err());
    }

    #[test]
    fn the_root_itself_and_duplicates_are_dropped() {
        let parsed = parse("/w", &["/w".to_owned(), "/other".to_owned(), "/other/".to_owned()]);
        assert_eq!(parsed.unwrap(), vec!["/other".to_owned()]);
    }

    #[test]
    fn grants_carry_permissions_and_metadata() {
        let (permissions, metadata) = grant(&["/other".to_owned()]);
        let permissions = permissions.expect("permissions");
        assert_eq!(
            permissions,
            json!([{ "action": "external_directory", "resource": "/other/*", "effect": "allow" }])
        );
        assert_eq!(metadata.expect("metadata")[KEY], json!(["/other"]));
    }

    #[test]
    fn empty_grants_are_noops() {
        let (permissions, metadata) = grant(&[]);
        assert!(permissions.is_none());
        assert!(metadata.is_none());
    }

    #[test]
    fn activate_replaces_only_the_owned_rules() {
        let foreign = json!({ "action": "bash", "resource": "*", "effect": "ask" });
        let previous_metadata = json!({ KEY: ["/old"] });
        let previous_permissions = json!([{ "action": "external_directory", "resource": "/old/*", "effect": "allow" }, foreign]);
        let body = activate(
            "ses_1",
            previous_permissions.as_array().map(Vec::as_slice),
            Some(&previous_metadata),
            &["/new".to_owned()],
        )
        .expect("activation");
        let permissions = body["permissions"].as_array().expect("rules");
        assert_eq!(permissions.len(), 2);
        assert_eq!(permissions[0]["resource"], "/new/*");
        assert_eq!(permissions[1], foreign);
        let metadata = body["metadata"].as_object().expect("metadata");
        assert_eq!(metadata[KEY], json!(["/new"]));
    }

    #[test]
    fn activate_without_changes_is_skipped() {
        let metadata = json!({ KEY: ["/other"] });
        let permissions = rules(&["/other".to_owned()]);
        assert!(activate(
            "ses_1",
            permissions.as_array().map(Vec::as_slice),
            Some(&metadata),
            &["/other".to_owned()]
        )
        .is_none());
    }

    #[test]
    fn list_reads_the_metadata_slot() {
        assert_eq!(list(Some(&json!({ KEY: ["/a", "/b"] }))), vec!["/a", "/b"]);
        assert_eq!(list(None).len(), 0, "no metadata ⇒ no directories");
        assert_eq!(list(Some(&json!({}))).len(), 0, "missing key ⇒ no directories");
    }
}
