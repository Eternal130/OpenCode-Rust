//! In-memory driver — port of `packages/core/src/environment/memory.ts`.
//!
//! A logical POSIX tree with symlink resolution, a failing spawner, and an
//! extra `symlink(target, path)` helper for tests.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::environment::driver::{CommandSpec, Driver, ProcessOutput, SpawnError, Spawner};
use crate::environment::files::{
    DirEntry, FileInfo, FileType, Files, FilesError, Range, ReadResult,
};

#[derive(Debug, Clone)]
enum Node {
    File { bytes: Vec<u8>, mtime_ms: f64 },
    Directory { mtime_ms: f64 },
    Symlink { target: String, mtime_ms: f64 },
}

impl Node {
    fn kind(&self) -> FileType {
        match self {
            Node::File { .. } => FileType::File,
            Node::Directory { .. } => FileType::Directory,
            Node::Symlink { .. } => FileType::Symlink,
        }
    }

    fn info(&self) -> FileInfo {
        FileInfo {
            kind: self.kind(),
            size: match self {
                Node::File { bytes, .. } => bytes.len() as u64,
                Node::Symlink { target, .. } => target.len() as u64,
                Node::Directory { .. } => 0,
            },
            mtime_ms: match self {
                Node::File { mtime_ms, .. }
                | Node::Directory { mtime_ms }
                | Node::Symlink { mtime_ms, .. } => *mtime_ms,
            },
        }
    }
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64() * 1_000.0)
        .unwrap_or(0.0)
}

/// `path.posix.resolve("/", value)` — lexical normalization to an absolute
/// POSIX path.
pub fn normalize(value: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("/{}", parts.join("/"))
}

fn dirname(value: &str) -> String {
    let normalized = normalize(value);
    match normalized.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(index) => normalized[..index].to_string(),
    }
}

fn basename(value: &str) -> String {
    let normalized = normalize(value);
    normalized.rsplit('/').next().unwrap_or_default().to_string()
}

fn join(base: &str, part: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), part)
}

type Nodes = HashMap<String, Node>;

fn resolve_seen(
    nodes: &Nodes,
    value: &str,
    follow_final: bool,
    seen: &mut Vec<String>,
) -> Option<String> {
    let normalized = normalize(value);
    let parts: Vec<String> =
        normalized.split('/').filter(|part| !part.is_empty()).map(str::to_string).collect();
    walk(nodes, "/", &parts, 0, follow_final, seen)
}

#[allow(clippy::too_many_arguments)]
fn walk(
    nodes: &Nodes,
    current: &str,
    parts: &[String],
    index: usize,
    follow_final: bool,
    seen: &mut Vec<String>,
) -> Option<String> {
    if index == parts.len() {
        return Some(current.to_string());
    }
    let part = &parts[index];
    let candidate = join(current, part);
    let node = nodes.get(&candidate);
    let is_symlink = matches!(node, Some(Node::Symlink { .. }));
    if !is_symlink || (!follow_final && index == parts.len() - 1) {
        return walk(nodes, &candidate, parts, index + 1, follow_final, seen);
    }
    if seen.contains(&candidate) {
        return None;
    }
    seen.push(candidate.clone());
    let target = match node {
        Some(Node::Symlink { target, .. }) => target.clone(),
        _ => return None,
    };
    let target_abs = normalize(&join(&dirname(&candidate), &target));
    let rest = parts[index + 1..].join("/");
    let combined = if rest.is_empty() {
        target_abs
    } else {
        format!("{}/{}", target_abs.trim_end_matches('/'), rest)
    };
    resolve_seen(nodes, &combined, follow_final, seen)
}

fn resolve_key(nodes: &Nodes, value: &str, follow_final: bool) -> Option<String> {
    resolve_seen(nodes, value, follow_final, &mut Vec::new())
}

/// The shared in-memory tree; implements both [`Files`] (as driver override)
/// and [`Spawner`] (always failing).
pub struct MemoryFs {
    nodes: Mutex<Nodes>,
}

impl MemoryFs {
    fn new() -> Self {
        let mut nodes = Nodes::new();
        nodes.insert("/".to_string(), Node::Directory { mtime_ms: now_ms() });
        MemoryFs { nodes: Mutex::new(nodes) }
    }

    fn lookup(nodes: &Nodes, value: &str) -> Option<Node> {
        let key = resolve_key(nodes, value, false).unwrap_or_else(|| normalize(value));
        nodes.get(&key).cloned()
    }

    fn require_parent(nodes: &Nodes, value: &str) -> Result<(), FilesError> {
        let parent_path = dirname(&normalize(value));
        let key = resolve_key(nodes, &parent_path, true).unwrap_or(parent_path);
        match nodes.get(&key) {
            None => Err(FilesError::failed(dirname(value), "Parent directory does not exist")),
            Some(Node::Directory { .. }) => Ok(()),
            Some(other) => Err(FilesError::failed(
                dirname(value),
                format!("Parent is not a directory (it is {:?})", other.kind()),
            )),
        }
    }

    fn mkdir_sync(nodes: &mut Nodes, value: &str) -> Result<(), FilesError> {
        let target = resolve_key(nodes, value, false).unwrap_or_else(|| normalize(value));
        match nodes.get(&target) {
            Some(Node::Directory { .. }) => return Ok(()),
            Some(other) => {
                return Err(FilesError::failed(
                    value,
                    format!("Path is not a directory (it is {:?})", other.kind()),
                ));
            }
            None => {}
        }
        let parent = dirname(&target);
        if parent != target {
            Self::mkdir_sync(nodes, &parent)?;
        }
        nodes.insert(target, Node::Directory { mtime_ms: now_ms() });
        Ok(())
    }

    /// Extra test helper: create a symlink at `path` pointing at `target`.
    pub async fn symlink(&self, target: &str, path: &str) -> Result<(), FilesError> {
        let mut nodes = self.nodes.lock();
        Self::require_parent(&nodes, path)?;
        let key = resolve_key(&nodes, path, false).unwrap_or_else(|| normalize(path));
        nodes.insert(key, Node::Symlink { target: target.to_string(), mtime_ms: now_ms() });
        Ok(())
    }
}

#[async_trait]
impl Files for MemoryFs {
    async fn stat(&self, value: &str) -> Result<FileInfo, FilesError> {
        let nodes = self.nodes.lock();
        match Self::lookup(&nodes, value) {
            Some(node) => Ok(node.info()),
            None => Err(FilesError::NotFound { path: value.to_string() }),
        }
    }

    async fn read(&self, value: &str, range: Option<Range>) -> Result<ReadResult, FilesError> {
        let nodes = self.nodes.lock();
        let original = match Self::lookup(&nodes, value) {
            None => return Err(FilesError::NotFound { path: value.to_string() }),
            Some(node) => node,
        };
        if original.kind() == FileType::Directory {
            return Err(FilesError::WrongKind {
                path: value.to_string(),
                actual: FileType::Directory,
            });
        }
        let resolved = resolve_key(&nodes, value, true).and_then(|key| nodes.get(&key).cloned());
        let node = match resolved {
            None => return Err(FilesError::NotFound { path: value.to_string() }),
            Some(node) => node,
        };
        if node.kind() != FileType::File {
            return Err(FilesError::WrongKind { path: value.to_string(), actual: node.kind() });
        }
        let Node::File { bytes, .. } = &node else { unreachable!() };
        let slice = match range {
            None => bytes.clone(),
            Some(range) => {
                let start = (range.offset as usize).min(bytes.len());
                let end = start.saturating_add(range.length as usize).min(bytes.len());
                bytes[start..end].to_vec()
            }
        };
        Ok(ReadResult { info: node.info(), bytes: slice })
    }

    async fn write(&self, value: &str, bytes: &[u8]) -> Result<(), FilesError> {
        let mut nodes = self.nodes.lock();
        let result = (|| -> Result<(), FilesError> {
            Self::mkdir_sync(&mut nodes, &dirname(&normalize(value)))?;
            let existing = Self::lookup(&nodes, value);
            if existing.as_ref().is_some_and(|node| node.kind() == FileType::Directory) {
                return Err(FilesError::failed(value, "Path is a directory"));
            }
            let target = match &existing {
                Some(Node::Symlink { .. }) => resolve_key(&nodes, value, true),
                _ => resolve_key(&nodes, value, false),
            }
            .ok_or_else(|| FilesError::failed(value, "Cannot resolve symlink"))?;
            Self::require_parent(&nodes, &target)?;
            nodes.insert(target, Node::File { bytes: bytes.to_vec(), mtime_ms: now_ms() });
            Ok(())
        })();
        result
    }

    async fn list(&self, value: &str) -> Result<Vec<DirEntry>, FilesError> {
        let nodes = self.nodes.lock();
        let target = resolve_key(&nodes, value, true).unwrap_or_else(|| normalize(value));
        let node = match nodes.get(&target) {
            None => return Err(FilesError::NotFound { path: value.to_string() }),
            Some(node) => node,
        };
        if node.kind() != FileType::Directory {
            return Err(FilesError::WrongKind { path: value.to_string(), actual: node.kind() });
        }
        let mut entries: Vec<DirEntry> = nodes
            .iter()
            .filter(|(entry, _)| entry.as_str() != target && dirname(entry) == target)
            .map(|(entry, child)| DirEntry { name: basename(entry), kind: child.kind() })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    async fn remove(&self, value: &str) -> Result<(), FilesError> {
        let mut nodes = self.nodes.lock();
        let target = resolve_key(&nodes, value, false).unwrap_or_else(|| normalize(value));
        nodes.retain(|entry, _| entry != &target && !entry.starts_with(&format!("{target}/")));
        Ok(())
    }

    async fn r#move(&self, from: &str, to: &str) -> Result<(), FilesError> {
        let mut nodes = self.nodes.lock();
        let source = resolve_key(&nodes, from, false).unwrap_or_else(|| normalize(from));
        let node = match nodes.get(&source) {
            None => return Err(FilesError::NotFound { path: from.to_string() }),
            Some(node) => node.clone(),
        };
        let result = (|| -> Result<(), FilesError> {
            let requested = resolve_key(&nodes, to, false).unwrap_or_else(|| normalize(to));
            let destination = if matches!(nodes.get(&requested), Some(Node::Directory { .. })) {
                join(&requested, &basename(&source))
            } else {
                requested
            };
            if node.kind() == FileType::Directory && destination.starts_with(&format!("{source}/"))
            {
                return Err(FilesError::failed(from, "Cannot move a directory into itself"));
            }
            if let Some(existing) = nodes.get(&destination) {
                if node.kind() == FileType::Directory && existing.kind() != FileType::Directory {
                    return Err(FilesError::failed(
                        to,
                        "Cannot overwrite a non-directory with a directory",
                    ));
                }
            }
            Self::require_parent(&nodes, &destination)?;
            let prefix = format!("{source}/");
            let moved: Vec<(String, Node)> = nodes
                .iter()
                .filter(|(entry, _)| entry.as_str() == source || entry.starts_with(&prefix))
                .map(|(entry, child)| (entry.clone(), child.clone()))
                .collect();
            for (entry, _) in &moved {
                nodes.remove(entry);
            }
            for (entry, child) in moved {
                nodes.insert(format!("{destination}{}", &entry[source.len()..]), child);
            }
            Ok(())
        })();
        result
    }

    async fn mkdir(&self, value: &str) -> Result<(), FilesError> {
        let mut nodes = self.nodes.lock();
        Self::mkdir_sync(&mut nodes, value)
    }
}

#[async_trait]
impl Spawner for MemoryFs {
    async fn spawn(&self, _command: CommandSpec) -> Result<ProcessOutput, SpawnError> {
        Err("The memory driver cannot spawn processes".to_string())
    }
}

/// `makeMemoryDriver()` — the tree plus the derived [`Driver`].
#[derive(Clone)]
pub struct MemoryDriver {
    fs: Arc<MemoryFs>,
}

impl Default for MemoryDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryDriver {
    pub fn new() -> Self {
        MemoryDriver { fs: Arc::new(MemoryFs::new()) }
    }

    /// The `Driver` view: failing spawner + in-memory `Files` overrides.
    pub fn driver(&self) -> Driver {
        Driver {
            spawner: self.fs.clone(),
            overrides: Some(self.fs.clone()),
            stdio: std::sync::Arc::new(crate::environment::stdio::FailingStdioSpawner(
                "The memory driver cannot spawn processes",
            )),
        }
    }

    /// Extra helper: `symlink(target, path)`.
    pub async fn symlink(&self, target: &str, path: &str) -> Result<(), FilesError> {
        self.fs.symlink(target, path).await
    }
}
