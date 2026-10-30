//! Environment file contract — port of `packages/core/src/environment/files.ts`.
//!
//! Content operations (`read`, `list`) follow final symlinks; metadata
//! operations (`stat` and entry tags returned by `list`) do not. `info`
//! describes the target file whose bytes are returned. The process-backed
//! default caps collected output at 64 MiB; larger whole-file reads fail with
//! [`FilesError::Failed`], so callers must use ranges for larger files.

use async_trait::async_trait;

/// `FileType` literal union: `file | directory | symlink | other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File,
    Directory,
    Symlink,
    Other,
}

/// Byte range for partial reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub offset: u64,
    pub length: u64,
}

/// `FileInfo`: lstat-or-follow metadata depending on the operation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FileInfo {
    pub kind: FileType,
    pub size: u64,
    pub mtime_ms: f64,
}

/// A directory entry with its own (non-followed) type tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub kind: FileType,
}

/// Error taxonomy: `Environment.NotFound | Environment.WrongKind | Environment.Failed`.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FilesError {
    #[error("not found: {path}")]
    NotFound { path: String },
    #[error("wrong kind at {path}: {actual:?}")]
    WrongKind { path: String, actual: FileType },
    #[error("failed at {path}: {cause}")]
    Failed { path: String, cause: String },
}

impl FilesError {
    pub fn failed(path: impl Into<String>, cause: impl std::fmt::Display) -> Self {
        FilesError::Failed { path: path.into(), cause: cause.to_string() }
    }
}

/// `read` result: metadata of the target file plus its bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadResult {
    pub info: FileInfo,
    pub bytes: Vec<u8>,
}

/// `FilesImpl` / `Files` contract.
#[async_trait]
pub trait Files: Send + Sync {
    async fn read(&self, path: &str, range: Option<Range>) -> Result<ReadResult, FilesError>;
    async fn write(&self, path: &str, bytes: &[u8]) -> Result<(), FilesError>;
    /// Describes the path entry itself, so a final symlink is reported as
    /// `symlink` rather than followed.
    async fn stat(&self, path: &str) -> Result<FileInfo, FilesError>;
    /// Follows a final symlink to the listed directory while preserving each
    /// returned entry's own type.
    async fn list(&self, path: &str) -> Result<Vec<DirEntry>, FilesError>;
    async fn remove(&self, path: &str) -> Result<(), FilesError>;
    async fn r#move(&self, from: &str, to: &str) -> Result<(), FilesError>;
    async fn mkdir(&self, path: &str) -> Result<(), FilesError>;
}

/// Derives a follow-stat kind from the lstat-like [`Files`] contract. A
/// dangling symlink fails with [`FilesError::NotFound`].
pub async fn type_following(files: &dyn Files, path: &str) -> Result<FileType, FilesError> {
    let info = files.stat(path).await?;
    if info.kind != FileType::Symlink {
        return Ok(info.kind);
    }
    match files.read(path, Some(Range { offset: 0, length: 0 })).await {
        Ok(result) => Ok(result.info.kind),
        // Reading a symlinked directory reports WrongKind(actual=directory),
        // which is exactly the follow-kind we want.
        Err(FilesError::WrongKind { actual, .. }) => Ok(actual),
        Err(other) => Err(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_matches_tags() {
        assert_eq!(
            FilesError::NotFound { path: "/x".into() }.to_string(),
            "not found: /x"
        );
        assert_eq!(
            FilesError::WrongKind { path: "/x".into(), actual: FileType::Directory }.to_string(),
            "wrong kind at /x: Directory"
        );
    }
}
