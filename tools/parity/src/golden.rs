//! Golden-file comparison with inline update. Golden files live in
//! `tools/parity/goldens/`. When `PARITY_UPDATE=1` is set, every assertion
//! (re)writes its golden instead of comparing — the sanctioned way to refresh
//! expectations after an intentional behavior change.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

/// Whether golden updates are enabled (`PARITY_UPDATE=1`).
#[must_use]
pub fn update_mode() -> bool {
    std::env::var("PARITY_UPDATE").is_ok_and(|value| value == "1")
}

/// The golden directory (`tools/parity/goldens`).
#[must_use]
pub fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("goldens")
}

/// Compare (or in update mode, write) a JSON golden in an explicit directory.
/// The rendered form is `to_string_pretty` plus a trailing newline. A missing
/// golden is written on first run (bootstrap) — ship it in-tree afterwards.
pub fn assert_json_in(directory: &Path, name: &str, value: &Value) -> Result<()> {
    let path = directory.join(name);
    let rendered = format!("{}\n", serde_json::to_string_pretty(value)?);
    if update_mode() || !path.is_file() {
        std::fs::create_dir_all(directory)
            .with_context(|| format!("create {}", directory.display()))?;
        std::fs::write(&path, rendered.as_bytes())
            .with_context(|| format!("write {}", path.display()))?;
        return Ok(());
    }
    let existing =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    if existing == rendered {
        return Ok(());
    }
    let detail = match (
        serde_json::from_str::<Value>(&existing),
        serde_json::from_str::<Value>(&rendered),
    ) {
        (Ok(golden), Ok(actual)) => match crate::httpdiff::deep_diff(&golden, &actual) {
            Some(divergence) => format!("; {divergence}"),
            None => String::new(),
        },
        _ => String::new(),
    };
    anyhow::bail!(
        "golden mismatch for {name}{detail}\n  golden file: {} (refresh with PARITY_UPDATE=1)",
        path.display()
    )
}

/// Compare (or write) a JSON golden in the default golden directory.
pub fn assert_json(name: &str, value: &Value) -> Result<()> {
    assert_json_in(&dir(), name, value)
}

/// Compare (or write) a raw-bytes golden in an explicit directory.
pub fn assert_bytes_in(directory: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let path = directory.join(name);
    if update_mode() || !path.is_file() {
        std::fs::create_dir_all(directory)
            .with_context(|| format!("create {}", directory.display()))?;
        std::fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
        return Ok(());
    }
    let existing = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    if existing == bytes {
        return Ok(());
    }
    anyhow::bail!(
        "golden byte mismatch for {name} ({} golden bytes vs {} actual bytes); refresh with PARITY_UPDATE=1",
        existing.len(),
        bytes.len()
    )
}

/// Compare (or write) a raw-bytes golden in the default golden directory.
pub fn assert_bytes(name: &str, bytes: &[u8]) -> Result<()> {
    assert_bytes_in(&dir(), name, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("parity-golden-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_then_matches_json_golden() {
        let dir = temp_dir();
        let value = json!({"stable": true});
        // Missing golden bootstraps on first assert (update mode is off; the
        // file does not exist yet, so it is created).
        assert_json_in(&dir, "sample.json", &value).unwrap();
        assert_json_in(&dir, "sample.json", &value).unwrap();
        let on_disk = std::fs::read_to_string(dir.join("sample.json")).unwrap();
        assert_eq!(on_disk, "{\n  \"stable\": true\n}\n");
        // A changed value must fail with a divergence message.
        let error = assert_json_in(&dir, "sample.json", &json!({"stable": false})).unwrap_err();
        assert!(error.to_string().contains("golden mismatch"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bytes_golden_round_trips() {
        let dir = temp_dir();
        assert_bytes_in(&dir, "sample.bin", b"hello").unwrap();
        assert_bytes_in(&dir, "sample.bin", b"hello").unwrap();
        assert!(assert_bytes_in(&dir, "sample.bin", b"world").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
