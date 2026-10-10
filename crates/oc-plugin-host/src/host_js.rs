//! Materializing the embedded `host.js` into the per-host sandbox.

use std::io;
use std::path::{Path, PathBuf};

/// Write `source` to `<sandbox>/host.js`, creating the sandbox.
pub(crate) fn materialize(sandbox: &Path, source: &str) -> io::Result<PathBuf> {
    std::fs::create_dir_all(sandbox)?;
    let path = sandbox.join("host.js");
    std::fs::write(&path, source)?;
    Ok(path)
}
