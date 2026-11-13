//! `config/worktree.ts` — `Config.Worktree`.

use serde::{Deserialize, Serialize};

/// `Config.Worktree`.
///
/// `directory` mirrors ref's `Schema.Trim.pipe(Schema.check(Schema.isNonEmpty()))`
/// (G-414): the value is trimmed on decode and an all-whitespace/empty
/// string fails deserialization, so normalize's `decode_encoded` drops
/// the entry with an `invalid` diagnostic — exactly ref's
/// `decodeEncoded` → `invalid(path)` → skip behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Worktree {
    /// Parent directory for new worktrees, relative to the project's primary
    /// checkout when not absolute.
    pub directory: String,
}

impl<'de> Deserialize<'de> for Worktree {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Raw {
            directory: String,
        }
        let Raw { directory } = Raw::deserialize(deserializer)?;
        let directory = directory.trim().to_string();
        if directory.is_empty() {
            return Err(serde::de::Error::custom(
                "worktree.directory must be a non-empty string after trimming",
            ));
        }
        Ok(Self { directory })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_surrounding_whitespace() {
        let parsed: Worktree =
            serde_json::from_value(serde_json::json!({"directory": " ../wt "})).unwrap();
        assert_eq!(parsed.directory, "../wt");
    }

    #[test]
    fn rejects_whitespace_only_and_empty() {
        for bad in [" ", "", "\t\n"] {
            assert!(
                serde_json::from_value::<Worktree>(serde_json::json!({ "directory": bad }))
                    .is_err(),
                "expected rejection for {bad:?}"
            );
        }
    }
}
