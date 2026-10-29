//! Port of `packages/core/src/repository.ts` (v2.0.24): remote/file
//! reference parsing, branch validation, and cache-path derivation.

use std::path::{Path, PathBuf};

/// A remote git reference (`Repository.RemoteReference`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteReference {
    pub host: String,
    pub path: String,
    pub segments: Vec<String>,
    pub owner: Option<String>,
    pub repo: String,
    pub remote: String,
    pub label: String,
    pub protocol: Option<String>,
}

/// A local `file://` reference (`Repository.FileReference`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReference {
    pub path: PathBuf,
    pub segments: Vec<String>,
    pub repo: String,
    pub remote: String,
    pub label: PathBuf,
}

/// `Repository.Reference`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference {
    Remote(RemoteReference),
    File(FileReference),
}

/// `RepositoryInvalidReferenceError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct InvalidReferenceError {
    pub repository: String,
    pub message: String,
}

/// `RepositoryUnsupportedLocalRepositoryError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct UnsupportedLocalRepositoryError {
    pub repository: String,
    pub message: String,
}

/// `RepositoryInvalidBranchError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct InvalidBranchError {
    pub branch: String,
    pub message: String,
}

/// Every parse/branch failure (`InvalidReferenceError | UnsupportedLocal… |
/// InvalidBranchError`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepositoryError {
    #[error(transparent)]
    InvalidReference(InvalidReferenceError),
    #[error(transparent)]
    UnsupportedLocal(UnsupportedLocalRepositoryError),
    #[error(transparent)]
    InvalidBranch(InvalidBranchError),
}

/// `Repository.parse` — `Some` only for inputs that build a safe reference.
#[must_use]
pub fn parse(input: &str) -> Option<Reference> {
    let cleaned = normalize_input(input);
    if cleaned.is_empty() {
        return None;
    }

    // `github:owner/repo` shorthand.
    if let Some(rest) = cleaned.strip_prefix("github:") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2
            && !parts[0].is_empty()
            && !parts[1].is_empty()
            && !parts[0].contains(char::is_whitespace)
            && !parts[1].contains(char::is_whitespace)
        {
            return build_remote(&parts[0].to_owned(), &[parts[1].to_owned()], None, None)
                .map(Reference::Remote);
        }
    }

    if !cleaned.contains("://") {
        // SCP-like `git@host:path` (at most one `@` before the host).
        if let Some((host, path)) = scp_split(&cleaned) {
            return build_remote(&host, &parts(&path), Some(&cleaned), None).map(Reference::Remote);
        }
        let direct = parts(&cleaned);
        if direct.len() >= 2 && host_like(&direct[0]) {
            return build_remote(&direct[0], &direct[1..], None, None).map(Reference::Remote);
        }
        if direct.len() == 2 {
            return build_remote(&"github.com".to_owned(), &direct, None, None)
                .map(Reference::Remote);
        }
    }

    // URL forms: `<scheme>://[user@]host[:port]/path…`.
    let (scheme, rest) = cleaned.split_once("://")?;
    if scheme == "file" {
        return build_file(rest, &cleaned);
    }
    let (authority, pathname) = match rest.split_once('/') {
        Some((authority, pathname)) => (authority, format!("/{pathname}")),
        None => (rest, String::new()),
    };
    let host_raw = authority.rsplit('@').next().unwrap_or(authority);
    let host = percent_decode(host_raw).to_lowercase();
    let segments = parts(&pathname);
    let github = host == "github.com";
    let remote = if github { github_remote(&segments.join("/")) } else { cleaned.clone() };
    build_remote(&host, &segments, Some(&remote), Some(&format!("{scheme}:")))
        .map(Reference::Remote)
}

/// `Repository.parseRemote` — throws on unparsable or local inputs.
pub fn parse_remote(input: &str) -> Result<RemoteReference, RepositoryError> {
    match parse(input) {
        Some(Reference::Remote(reference)) => Ok(reference),
        Some(Reference::File(_)) => {
            Err(RepositoryError::UnsupportedLocal(UnsupportedLocalRepositoryError {
                repository: input.to_owned(),
                message: "Local file repositories are not supported".to_owned(),
            }))
        }
        None => Err(RepositoryError::InvalidReference(InvalidReferenceError {
            repository: input.to_owned(),
            message:
                "Repository must be a git URL, host/path reference, or GitHub owner/repo shorthand"
                    .to_owned(),
        })),
    }
}

/// `Repository.validateBranch` — `[A-Za-z0-9/_.-]+`, no leading `-`, no `..`.
pub fn validate_branch(branch: &str) -> Result<(), InvalidBranchError> {
    let valid = !branch.is_empty()
        && !branch.starts_with('-')
        && !branch.contains("..")
        && branch.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '.' | '-'));
    if valid {
        Ok(())
    } else {
        Err(InvalidBranchError {
            branch: branch.to_owned(),
            message: "Branch must contain only alphanumeric characters, /, _, ., and -, and cannot start with - or contain .."
                .to_owned(),
        })
    }
}

/// `Repository.isFile`.
#[must_use]
pub fn is_file(reference: &Reference) -> bool {
    matches!(reference, Reference::File(_))
}

/// `Repository.isRemote`.
#[must_use]
pub fn is_remote(reference: &Reference) -> bool {
    !is_file(reference)
}

/// `Repository.cachePath` — checkouts key by remote + branch; the branch is
/// percent-encoded because valid names may contain `/`.
#[must_use]
pub fn cache_path(root: &Path, reference: &Reference, branch: Option<&str>) -> PathBuf {
    let (host, segments) = match reference {
        Reference::Remote(remote) => (remote.host.clone(), remote.segments.clone()),
        Reference::File(file) => ("file".to_owned(), file.segments.clone()),
    };
    let mut base = root.to_path_buf();
    for part in host.split(':') {
        base = base.join(part);
    }
    for segment in segments {
        base = base.join(segment);
    }
    match branch {
        Some(branch) => {
            let mut name = base.into_os_string();
            name.push(format!("@{}", urlencoding::encode(branch)));
            PathBuf::from(name)
        }
        None => base,
    }
}

/// `Repository.cacheIdentity` — `host/path`.
#[must_use]
pub fn cache_identity(reference: &Reference) -> String {
    match reference {
        Reference::Remote(remote) => format!("{}/{}", remote.host, remote.path),
        Reference::File(file) => format!("file/{}", file.path.to_string_lossy()),
    }
}

/// `Repository.same`.
#[must_use]
pub fn same(left: &Reference, right: &Reference) -> bool {
    cache_identity(left) == cache_identity(right)
}

// ---------------------------------------------------------------------------
// Builders (repository.ts:163-204)
// ---------------------------------------------------------------------------

fn build_remote(
    host: &str,
    segments: &[String],
    remote: Option<&str>,
    protocol: Option<&str>,
) -> Option<RemoteReference> {
    let segments: Vec<String> = segments
        .iter()
        .map(|segment| trim_git_suffix(segment))
        .filter(|segment| !segment.is_empty())
        .collect();
    if !safe_host(host)
        || segments.is_empty()
        || segments.iter().any(|segment| !safe_segment(segment))
    {
        return None;
    }
    let repository_path = segments.join("/");
    let host = host.to_lowercase();
    let github = host == "github.com";
    Some(RemoteReference {
        owner: segments.first().filter(|_| segments.len() == 2).cloned(),
        repo: segments.last().cloned().unwrap_or_default(),
        remote: remote.map(str::to_owned).unwrap_or_else(|| {
            if github {
                github_remote(&repository_path)
            } else {
                format!("https://{host}/{repository_path}.git")
            }
        }),
        label: if github && segments.len() == 2 {
            repository_path.clone()
        } else {
            format!("{host}/{repository_path}")
        },
        path: repository_path,
        host,
        segments,
        protocol: protocol.map(str::to_owned),
    })
}

fn build_file(rest: &str, remote: &str) -> Option<Reference> {
    // `file://` URLs: an empty authority precedes the absolute path.
    let raw = if rest.starts_with('/') { rest.to_owned() } else { format!("/{rest}") };
    let file_path = std::fs::canonicalize(&raw).unwrap_or_else(|_| PathBuf::from(&raw));
    let segments: Vec<String> = file_path
        .to_string_lossy()
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.trim_end_matches(':').to_owned())
        .collect();
    if segments.is_empty() {
        return None;
    }
    let repo = trim_git_suffix(segments.last().map(String::as_str).unwrap_or(""));
    Some(Reference::File(FileReference {
        segments,
        repo,
        label: file_path.clone(),
        path: file_path,
        remote: remote.to_owned(),
    }))
}

/// `githubRemote` (repository.ts:166-171) — `OPENCODE_REPO_CLONE_GITHUB_BASE_URL`
/// override supported.
fn github_remote(pathname: &str) -> String {
    match std::env::var("OPENCODE_REPO_CLONE_GITHUB_BASE_URL") {
        Ok(base) if !base.is_empty() => format!("{}/{}.git", base.trim_end_matches('/'), pathname),
        _ => format!("https://github.com/{pathname}.git"),
    }
}

// ---------------------------------------------------------------------------
// Lexing (repository.ts:117-161)
// ---------------------------------------------------------------------------

fn normalize_input(input: &str) -> String {
    let trimmed = input.trim();
    let no_plus = trimmed.strip_prefix("git+").unwrap_or(trimmed);
    let no_fragment = no_plus.split('#').next().unwrap_or("");
    no_fragment.trim_end_matches('/').to_owned()
}

fn trim_git_suffix(input: &str) -> String {
    input.strip_suffix(".git").unwrap_or(input).to_owned()
}

fn parts(input: &str) -> Vec<String> {
    input
        .split('/')
        .map(|item| trim_git_suffix(item.trim()))
        .filter(|item| !item.is_empty())
        .collect()
}

/// SCP split `^(?:[^@/\s]+@)?([^:/\s]+):(.+)$` — host before the first
/// colon when the input has no `/` before it and no scheme.
fn scp_split(input: &str) -> Option<(String, String)> {
    let body = match input.split_once('@') {
        Some((user, rest))
            if !user.is_empty() && !user.contains('/') && !user.contains(char::is_whitespace) =>
        {
            rest
        }
        _ => input,
    };
    let (host, path) = body.split_once(':')?;
    if host.is_empty()
        || path.is_empty()
        || host.contains('/')
        || host.contains(char::is_whitespace)
        || host.contains(':')
    {
        return None;
    }
    Some((host.to_owned(), path.to_owned()))
}

fn host_like(input: &str) -> bool {
    input.contains('.') || input.contains(':') || input == "localhost"
}

/// `safeHost` — every `:`-separated part must be a safe segment.
fn safe_host(input: &str) -> bool {
    !input.is_empty() && !input.starts_with('-') && input.split(':').all(safe_segment)
}

fn safe_segment(input: &str) -> bool {
    input != "." && input != ".." && !input.contains(':') && !input.contains([' ', '/', '\\'])
}

fn percent_decode(input: &str) -> String {
    urlencoding::decode(input).map(|value| value.into_owned()).unwrap_or_else(|_| input.to_owned())
}
