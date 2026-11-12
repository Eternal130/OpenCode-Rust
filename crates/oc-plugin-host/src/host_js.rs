//! Materializing the embedded `host.js` into the per-host sandbox.

use std::io;
use std::path::{Path, PathBuf};

/// A `package.json` pinned to ESM, written next to host.js so the
/// module semantics are deterministic under BOTH runtime tracks
/// (G-333: bun and node both treat the sandbox as `"type": "module"`;
/// bun is unaffected — it already ran the file as ESM).
const PACKAGE_JSON: &str = "{\n  \"type\": \"module\"\n}\n";

/// Write `source` to `<sandbox>/host.js` (plus the ESM-pinning
/// `package.json`), creating the sandbox.
pub(crate) fn materialize(sandbox: &Path, source: &str) -> io::Result<PathBuf> {
    std::fs::create_dir_all(sandbox)?;
    let path = sandbox.join("host.js");
    std::fs::write(&path, source)?;
    std::fs::write(sandbox.join("package.json"), PACKAGE_JSON)?;
    Ok(path)
}
