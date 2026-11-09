//! Ports of the v2 reference test suites: `repository.test.ts`,
//! `repository-cache.test.ts` (local fixture git repos — never network),
//! `reference.test.ts`, and `reference-instructions.test.ts`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::repository::{
    cache_identity, cache_path, is_file, is_remote, parse, parse_remote, same, validate_branch,
    Reference,
};
use super::repository_cache::{
    CacheError, EnsureInput, MemoryRefreshStore, Refresh, RefreshStore, RepositoryCache,
};
use super::{GitSource, Info, LocalSource, ReferenceService, Source};
use oc_util::determinism::Clock;

// ---------------------------------------------------------------------------
// repository.test.ts
// ---------------------------------------------------------------------------

#[test]
fn repository_parses_github_shorthand_and_cache_paths() {
    let reference = parse_remote("owner/repo").unwrap();
    assert_eq!(reference.host, "github.com");
    assert_eq!(reference.path, "owner/repo");
    assert_eq!(reference.segments, ["owner", "repo"]);
    assert_eq!(reference.owner.as_deref(), Some("owner"));
    assert_eq!(reference.repo, "repo");
    assert_eq!(reference.remote, "https://github.com/owner/repo.git");
    assert_eq!(reference.label, "owner/repo");

    assert_eq!(
        cache_path(Path::new("/cache"), &Reference::Remote(reference.clone()), None),
        PathBuf::from("/cache/github.com/owner/repo")
    );
    assert_eq!(
        cache_path(Path::new("/cache"), &Reference::Remote(reference.clone()), Some("main")),
        PathBuf::from("/cache/github.com/owner/repo@main")
    );
    assert_eq!(
        cache_path(Path::new("/cache"), &Reference::Remote(reference.clone()), Some("feature/x")),
        PathBuf::from("/cache/github.com/owner/repo@feature%2Fx")
    );
    assert_eq!(cache_identity(&Reference::Remote(reference)), "github.com/owner/repo");
}

#[test]
fn repository_parses_host_paths_and_scp_remotes() {
    let hosted = parse_remote("gitlab.com/group/repo").unwrap();
    assert_eq!(hosted.host, "gitlab.com");
    assert_eq!(hosted.remote, "https://gitlab.com/group/repo.git");
    assert_eq!(hosted.label, "gitlab.com/group/repo");

    let scp = parse_remote("git@github.com:owner/repo.git").unwrap();
    assert_eq!(scp.host, "github.com");
    assert_eq!(scp.path, "owner/repo");
    assert_eq!(scp.remote, "git@github.com:owner/repo.git");
    assert_eq!(scp.label, "owner/repo");
}

#[test]
fn repository_caches_host_ports_per_directory_part() {
    let reference = parse_remote("ssh://git@example.com:2222/owner/repo").unwrap();
    assert_eq!(
        cache_path(Path::new("/cache"), &Reference::Remote(reference), None),
        PathBuf::from("/cache/example.com/2222/owner/repo")
    );
}

#[test]
fn repository_keeps_file_references_distinct() {
    let local_path = std::env::temp_dir().join("repo.git");
    let url = format!("file://{}", local_path.to_string_lossy());
    let reference = parse(&url).unwrap();
    assert!(is_file(&reference));
    assert!(!is_remote(&reference));
    let unsupported = parse_remote(&url).unwrap_err();
    assert!(matches!(unsupported, super::repository::RepositoryError::UnsupportedLocal(_)));
}

/// G-278③ — `file://host/path` is REJECTED (ref `fileURLToPath` throws
/// ERR_INVALID_FILE_URL_HOST → parse returns undefined); the host never
/// becomes a path segment.
#[test]
fn repository_rejects_file_urls_with_a_host() {
    assert!(parse("file://example.com/repo").is_none());
    assert!(parse("file://localhost/repo").is_some(), "localhost authority is allowed");
    // Percent-escapes decode; encoded separators reject.
    let reference = parse("file:///tmp/my%20repo").unwrap();
    assert!(
        matches!(&reference, super::repository::Reference::File(file) if file.path == PathBuf::from("/tmp/my repo"))
    );
    assert!(parse("file:///a%2Fb").is_none());
}

/// G-278③ — the file path folds `.`/`..` LEXICALLY (ref
/// `path.normalize(fileURLToPath(url))`); the filesystem is never
/// touched, so the cachePath key is stable text even for nonexistent
/// or symlinked paths.
#[test]
fn repository_file_paths_normalize_lexically() {
    let reference = parse("file:///tmp/a/../b/repo.git").unwrap();
    let super::repository::Reference::File(file) = &reference else { panic!() };
    assert_eq!(file.path, PathBuf::from("/tmp/b/repo.git"), ".. folds lexically");
    assert_eq!(file.repo, "repo");
    // A nonexistent path parses identically (no canonicalize).
    let missing = parse("file:///definitely/not/here/../here").unwrap();
    let super::repository::Reference::File(file) = &missing else { panic!() };
    assert_eq!(file.path, PathBuf::from("/definitely/not/here"));
}

#[test]
fn repository_rejects_unsafe_references_and_branches() {
    use super::repository::RepositoryError;
    assert!(matches!(parse_remote("not-a-repo"), Err(RepositoryError::InvalidReference(_))));
    assert!(matches!(
        parse_remote("git@github.com:../../../etc/passwd"),
        Err(RepositoryError::InvalidReference(_))
    ));

    assert!(validate_branch("feature/docs.v1").is_ok());
    assert!(validate_branch("-bad").is_err());
    assert!(validate_branch("bad..branch").is_err());
    assert!(validate_branch("bad branch").is_err());

    for input in [
        "..:repo",
        "git@..:repo",
        "../owner/repo",
        "ssh://../repo",
        "ssh://..:22/repo",
        "https://%2e%2e/owner/repo",
        ".:repo",
    ] {
        assert!(
            matches!(parse_remote(input), Err(RepositoryError::InvalidReference(_))),
            "{input} should be rejected"
        );
    }
}

#[test]
fn repository_compares_cache_identity_across_spellings() {
    let shorthand = parse_remote("owner/repo").unwrap();
    assert!(same(
        &Reference::Remote(shorthand.clone()),
        &Reference::Remote(parse_remote("https://github.com/owner/repo.git").unwrap())
    ));
    assert!(same(
        &Reference::Remote(shorthand),
        &Reference::Remote(parse_remote("github.com/owner/repo").unwrap())
    ));
}

// ---------------------------------------------------------------------------
// Fixture: a local origin repo (test/fixture/git.ts) — no network.
// ---------------------------------------------------------------------------

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    reference: super::repository::RemoteReference,
}

fn git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {args:?} in {path:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn with_remote() -> Fixture {
    let root = tempfile::tempdir().expect("tempdir");
    let origin = root.path().join("origin.git");
    let source = root.path().join("source");
    std::fs::create_dir_all(&source).unwrap();
    git(root.path(), &["init", "--bare", origin.to_str().unwrap()]);
    git(&source, &["init"]);
    git(&source, &["config", "user.email", "test@example.com"]);
    git(&source, &["config", "user.name", "Test"]);
    std::fs::write(source.join("README.md"), "one\n").unwrap();
    git(&source, &["add", "README.md"]);
    git(&source, &["commit", "-m", "initial"]);
    git(&source, &["branch", "-M", "main"]);
    git(&source, &["remote", "add", "origin", &origin_url(&origin)]);
    git(&source, &["push", "-u", "origin", "main"]);
    git(
        root.path(),
        &["--git-dir", origin.to_str().unwrap(), "symbolic-ref", "HEAD", "refs/heads/main"],
    );
    // The ref fixture overrides `remote` with the local origin URL (a
    // file:// remote clone source) while keeping the parsed owner/repo
    // identity — no network anywhere.
    let reference = super::repository::RemoteReference {
        remote: origin_url(&origin),
        ..parse_remote("owner/repo").expect("owner/repo parses")
    };
    // Keep the tempdir alive for the test body via leak (small, per-test).
    let root = root.keep();
    Fixture { root, source, reference }
}

fn origin_url(origin: &Path) -> String {
    format!("file://{}", origin.to_string_lossy())
}

fn commit(path: &Path, content: &str, message: &str) {
    std::fs::write(path.join("README.md"), content).unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-m", message]);
    git(path, &["push", "origin", "HEAD:main"]);
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// A clock the test advances manually (the injected daily-throttle clock).
struct MutableClock(SystemTime);

impl Clock for MutableClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}

fn cache_layer(
    fixture: &Fixture,
    store: Arc<dyn RefreshStore>,
    now: SystemTime,
) -> RepositoryCache {
    std::fs::create_dir_all(fixture.root.join("repos")).unwrap();
    RepositoryCache::new(fixture.root.join("repos"), Arc::new(MutableClock(now)), store)
}

fn daily(fixture: &Fixture) -> (RepositoryCache, Arc<MemoryRefreshStore>) {
    let store = Arc::new(MemoryRefreshStore::default());
    (
        cache_layer(
            fixture,
            Arc::clone(&store) as Arc<dyn RefreshStore>,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        ),
        store,
    )
}

fn ensure(reference: &super::repository::RemoteReference) -> EnsureInput {
    EnsureInput { reference: reference.clone(), branch: None, refresh: Refresh::Daily, depth: None }
}

// ---------------------------------------------------------------------------
// repository-cache.test.ts
// ---------------------------------------------------------------------------

#[test]
fn cache_persists_daily_throttle_and_serializes_refreshes() {
    let fixture = with_remote();
    let (cache, store) = daily(&fixture);
    let input = ensure(&fixture.reference);
    let initial = cache.ensure(&input).unwrap();
    assert_eq!(initial.status, "cloned");
    assert_eq!(read(&initial.local_path.join("README.md")), "one\n");

    commit(&fixture.source, "two\n", "advance main");
    // Fresh cache instance: the throttle record must persist (the ref test
    // recreates the layer between checks).
    let cache = cache_layer(
        &fixture,
        Arc::clone(&store) as Arc<dyn RefreshStore>,
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_100),
    );
    assert_eq!(cache.ensure(&ensure(&fixture.reference)).unwrap().status, "cached");
    let key = format!("repository-cache:{}", initial.local_path.to_string_lossy());
    store.set_attempted_at(&key, 1_700_000_100_000 - 86_400_000);

    let results: Vec<_> =
        [0, 1].iter().map(|_| cache.ensure(&ensure(&fixture.reference)).unwrap().status).collect();
    assert!(results.contains(&"refreshed"));
    assert!(results.contains(&"cached"));
    assert_eq!(read(&initial.local_path.join("README.md")), "two\n");

    // A missing checkout is recreated even with a fresh timestamp.
    std::fs::remove_dir_all(&initial.local_path).unwrap();
    let store2 = Arc::new(MemoryRefreshStore::default());
    store2.set_attempted_at(&key, 1_700_000_100_000);
    let cache =
        cache_layer(&fixture, store2, SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_100));
    assert_eq!(cache.ensure(&ensure(&fixture.reference)).unwrap().status, "cloned");
    assert_eq!(read(&initial.local_path.join("README.md")), "two\n");
}

#[test]
fn cache_allows_forced_refresh() {
    let fixture = with_remote();
    let (cache, _) = daily(&fixture);
    let input = ensure(&fixture.reference);
    let initial = cache.ensure(&input).unwrap();
    commit(&fixture.source, "two\n", "advance main");

    assert_eq!(
        cache
            .ensure(&EnsureInput { refresh: Refresh::Forced, ..ensure(&fixture.reference) })
            .unwrap()
            .status,
        "refreshed"
    );
    assert_eq!(read(&initial.local_path.join("README.md")), "two\n");
    assert_eq!(cache.ensure(&input).unwrap().status, "cached");
}

#[test]
fn cache_throttles_failed_refresh_attempts() {
    let fixture = with_remote();
    let (cache, store) = daily(&fixture);
    let input = ensure(&fixture.reference);
    let initial = cache.ensure(&input).unwrap();
    let key = format!("repository-cache:{}", initial.local_path.to_string_lossy());
    store.set_attempted_at(&key, 1_700_000_000_000 - 86_400_000);
    // Take the origin offline.
    let offline = fixture.root.join("offline.git");
    std::fs::rename(fixture.root.join("origin.git"), &offline).unwrap();

    let error = cache.ensure(&input).unwrap_err();
    assert!(matches!(error, CacheError::FetchFailed { .. }), "{error:?}");
    // The failed attempt counts: the next daily ensure stays cached.
    assert_eq!(cache.ensure(&input).unwrap().status, "cached");
    assert_eq!(read(&initial.local_path.join("README.md")), "one\n");

    std::fs::rename(&offline, fixture.root.join("origin.git")).unwrap();
    commit(&fixture.source, "two\n", "advance main");
    store.set_attempted_at(&key, 1_700_000_000_000 - 86_400_000);
    assert_eq!(cache.ensure(&input).unwrap().status, "refreshed");
    assert_eq!(read(&initial.local_path.join("README.md")), "two\n");
}

#[test]
fn cache_keeps_branch_freshness_independent() {
    let fixture = with_remote();
    // Create a feature branch on the origin.
    git(&fixture.source, &["checkout", "-b", "feature"]);
    std::fs::write(fixture.source.join("README.md"), "feature\n").unwrap();
    git(&fixture.source, &["add", "."]);
    git(&fixture.source, &["commit", "-m", "feature"]);
    git(&fixture.source, &["push", "origin", "feature"]);
    git(&fixture.source, &["checkout", "main"]);

    let (cache, store) = daily(&fixture);
    let main = cache.ensure(&ensure(&fixture.reference)).unwrap();
    let feature_input = EnsureInput {
        reference: fixture.reference.clone(),
        branch: Some("feature".to_owned()),
        refresh: Refresh::Daily,
        depth: None,
    };
    let feature = cache.ensure(&feature_input).unwrap();
    assert_eq!(read(&feature.local_path.join("README.md")), "feature\n");
    store.remove(&format!("repository-cache:{}", feature.local_path.to_string_lossy()));

    git(&fixture.source, &["checkout", "feature"]);
    std::fs::write(fixture.source.join("README.md"), "new feature\n").unwrap();
    git(&fixture.source, &["add", "."]);
    git(&fixture.source, &["commit", "-m", "advance feature"]);
    git(&fixture.source, &["push", "origin", "feature"]);
    git(&fixture.source, &["checkout", "main"]);

    assert_eq!(cache.ensure(&ensure(&fixture.reference)).unwrap().status, "cached");
    assert_eq!(cache.ensure(&feature_input).unwrap().status, "refreshed");
    assert_eq!(read(&main.local_path.join("README.md")), "one\n");
    assert_eq!(read(&feature.local_path.join("README.md")), "new feature\n");
}

#[test]
fn cache_replaces_stale_directory_before_cloning() {
    let fixture = with_remote();
    let repos = fixture.root.join("repos");
    let local_path = cache_path(&repos, &Reference::Remote(fixture.reference.clone()), None);
    std::fs::create_dir_all(&local_path).unwrap();
    std::fs::write(local_path.join("stale.txt"), "stale").unwrap();

    let (cache, _) = daily(&fixture);
    let result = cache
        .ensure(&EnsureInput {
            reference: fixture.reference.clone(),
            branch: None,
            refresh: Refresh::IfMissing,
            depth: None,
        })
        .unwrap();
    assert_eq!(result.status, "cloned");
    assert!(!local_path.join("stale.txt").exists());
    assert_eq!(read(&local_path.join("README.md")), "one\n");
}

#[test]
fn cache_rejects_invalid_branches() {
    let fixture = with_remote();
    let (cache, _) = daily(&fixture);
    let error = cache
        .ensure(&EnsureInput {
            reference: fixture.reference.clone(),
            branch: Some("../escape".to_owned()),
            refresh: Refresh::Daily,
            depth: None,
        })
        .unwrap_err();
    assert!(matches!(error, CacheError::InvalidBranch { .. }), "{error:?}");
}

// ---------------------------------------------------------------------------
// reference.test.ts
// ---------------------------------------------------------------------------

fn service(repos_root: &Path, cache: RepositoryCache) -> Arc<ReferenceService> {
    let mut service = ReferenceService::with_cache(repos_root.to_owned(), cache);
    service.set_refresh_on_mutation(false);
    Arc::new(service)
}

fn local_info(name: &str, path: &Path, description: Option<&str>) -> Info {
    Info {
        name: name.to_owned(),
        path: path.to_owned(),
        description: description.map(str::to_owned),
        hidden: None,
        source: Source::Local(LocalSource {
            path: path.to_owned(),
            description: description.map(str::to_owned),
            hidden: None,
        }),
    }
}

#[test]
fn reference_reads_the_editor_source_by_name() {
    let fixture = with_remote();
    let references = service(&fixture.root.join("repos"), daily(&fixture).0);
    let source = Source::Local(LocalSource {
        path: PathBuf::from("/docs"),
        description: None,
        hidden: None,
    });
    references.add("docs", source.clone());
    assert_eq!(references.get("docs"), Some(source.clone()));
    assert_eq!(references.get("missing"), None);
    let replacement = Source::Git(GitSource {
        repository: "owner/repo".to_owned(),
        branch: None,
        description: None,
        hidden: None,
    });
    references.add("docs", replacement.clone());
    assert_eq!(references.get("docs"), Some(replacement.clone()));
    references.remove("docs");
    assert_eq!(references.get("docs"), None);
    assert!(references.list().is_empty());
}

#[test]
fn reference_drops_invalid_git_sources_and_derives_cache_paths() {
    let fixture = with_remote();
    let repos = fixture.root.join("repos");
    std::fs::create_dir_all(&repos).unwrap();
    // A memory cache: list() derives paths without touching Git.
    let references = service(&repos, daily(&fixture).0);
    references.add(
        "docs",
        Source::Local(LocalSource {
            path: PathBuf::from("/docs"),
            description: None,
            hidden: None,
        }),
    );
    references.add(
        "sdk",
        Source::Git(GitSource {
            repository: "owner/repo".to_owned(),
            branch: Some("feature/docs".to_owned()),
            description: Some("SDK documentation".to_owned()),
            hidden: Some(true),
        }),
    );
    references.add(
        "invalid",
        Source::Git(GitSource {
            repository: "invalid".to_owned(),
            branch: None,
            description: None,
            hidden: None,
        }),
    );
    references.add(
        "invalid-branch",
        Source::Git(GitSource {
            repository: "owner/repo".to_owned(),
            branch: Some("../escape".to_owned()),
            description: None,
            hidden: None,
        }),
    );
    references.add(
        "file",
        Source::Git(GitSource {
            repository: "file:///docs".to_owned(),
            branch: None,
            description: None,
            hidden: None,
        }),
    );

    let infos = references.list();
    assert_eq!(infos.iter().map(|info| info.name.as_str()).collect::<Vec<_>>(), ["docs", "sdk"]);
    let expected_path = cache_path(
        &repos,
        &Reference::Remote(parse_remote("owner/repo").unwrap()),
        Some("feature/docs"),
    );
    assert_eq!(infos[1].path, expected_path);
    assert_eq!(infos[1].description.as_deref(), Some("SDK documentation"));
    assert_eq!(infos[1].hidden, Some(true));
}

#[test]
fn reference_preserves_configured_git_descriptions() {
    let fixture = with_remote();
    let repos = fixture.root.join("repos");
    let references = service(&repos, daily(&fixture).0);
    let repository = parse_remote("owner/repo").unwrap();
    references.add(
        "sdk",
        Source::Git(GitSource {
            repository: "owner/repo".to_owned(),
            branch: None,
            description: Some("Use for SDK implementation details".to_owned()),
            hidden: None,
        }),
    );
    assert_eq!(
        references.list(),
        vec![Info {
            name: "sdk".to_owned(),
            path: cache_path(&repos, &Reference::Remote(repository), None),
            description: Some("Use for SDK implementation details".to_owned()),
            hidden: None,
            source: references.get("sdk").unwrap(),
        }]
    );
}

#[test]
fn reference_refresh_ensures_git_sources_daily() {
    use super::repository_cache::CacheResult;
    use parking_lot::Mutex as PLMutex;

    // reference.test.ts "reads batched references before cache work": a
    // mocked cache records the ensure inputs.
    let operations: Arc<PLMutex<Vec<(String, Option<String>, Refresh)>>> =
        Arc::new(PLMutex::new(Vec::new()));
    let recorder = {
        let operations = Arc::clone(&operations);
        let repos = PathBuf::from("/repos");
        Arc::new(move |input: &EnsureInput| {
            operations.lock().push((
                input.reference.remote.clone(),
                input.branch.clone(),
                input.refresh,
            ));
            Ok(CacheResult {
                repository: input.reference.label.clone(),
                host: input.reference.host.clone(),
                remote: input.reference.remote.clone(),
                local_path: cache_path(
                    &repos,
                    &Reference::Remote(input.reference.clone()),
                    input.branch.as_deref(),
                ),
                status: "cached",
                head: None,
                branch: None,
            })
        }) as super::EnsureFn
    };
    let references = {
        let mut service = ReferenceService::with_ensure(PathBuf::from("/repos"), recorder);
        service.set_refresh_on_mutation(false);
        Arc::new(service)
    };
    references.add(
        "docs",
        Source::Local(LocalSource {
            path: PathBuf::from("/docs"),
            description: None,
            hidden: None,
        }),
    );
    references.add(
        "invalid",
        Source::Git(GitSource {
            repository: "invalid".to_owned(),
            branch: None,
            description: None,
            hidden: None,
        }),
    );
    references.add(
        "sdk",
        Source::Git(GitSource {
            repository: "owner/repo".to_owned(),
            branch: Some("feature/docs".to_owned()),
            description: None,
            hidden: None,
        }),
    );
    references.refresh_all();
    assert_eq!(
        operations.lock().as_slice(),
        [(
            "https://github.com/owner/repo.git".to_owned(),
            Some("feature/docs".to_owned()),
            Refresh::Daily
        )]
    );
    // Idempotent per sweep: another refresh re-ensures (the cache
    // throttles).
    references.refresh_all();
    assert_eq!(operations.lock().len(), 2);
}

// ---------------------------------------------------------------------------
// reference-instructions.test.ts
// ---------------------------------------------------------------------------

fn instructions_info(name: &str, description: Option<&str>) -> Info {
    let path = PathBuf::from(format!("/{name}"));
    local_info(name, &path, description)
}

#[test]
fn instructions_list_available_references() {
    let infos = vec![instructions_info("docs", Some("Use for product documentation"))];
    let text = super::instructions::render(&super::instructions::summaries(&infos));
    assert!(text.contains("<available_references>"));
    assert!(text.contains("<name>docs</name>"));
    assert!(text.contains("<path>/docs</path>"));
    assert!(text.contains("<description>Use for product documentation</description>"));
}

#[test]
fn instructions_omit_empty_and_undescribed_references() {
    assert!(super::instructions::summaries(&[]).is_empty());
    assert!(super::instructions::summaries(&[instructions_info("docs", None)]).is_empty());
}

#[test]
fn instructions_announce_added_and_removed_deltas() {
    let docs = || instructions_info("docs", Some("Use for product documentation"));
    let examples = || instructions_info("examples", Some("Use for examples"));

    let previous = super::instructions::summaries(&[docs()]);
    let current = super::instructions::summaries(&[docs(), examples()]);
    assert_eq!(
        super::instructions::update(&previous, &current),
        [
            "New project references are available in addition to those previously listed:",
            "  <reference>",
            "    <name>examples</name>",
            "    <path>/examples</path>",
            "    <description>Use for examples</description>",
            "  </reference>",
        ]
        .join("\n")
    );

    let after_removal = super::instructions::summaries(&[examples()]);
    assert_eq!(
        super::instructions::update(&current, &after_removal),
        "The following project references are no longer available and must not be used: docs."
    );
}

#[test]
fn instructions_restate_the_list_when_entries_change() {
    let docs = || instructions_info("docs", Some("old"));
    let changed = || instructions_info("docs", Some("new"));
    let previous = super::instructions::summaries(&[docs()]);
    let current = super::instructions::summaries(&[changed()]);
    let text = super::instructions::update(&previous, &current);
    assert!(text.starts_with(
        "The available project references have changed. This list supersedes the previous reference list."
    ));
    assert!(text.contains("<description>new</description>"));
    // No change at all still restates the full list.
    assert!(super::instructions::update(&previous, &previous).contains("<available_references>"));
}

// ---------------------------------------------------------------------------
// The context.rs `core/reference-guidance` instruction source.
// ---------------------------------------------------------------------------

#[test]
fn guidance_source_reads_renders_and_removes() {
    use crate::session::runner::context::{
        instruction_read, reference_guidance_source, render_initial, render_update,
        InstructionList, ReadOutcome,
    };
    use std::collections::BTreeMap;

    let infos: Arc<dyn Fn() -> Vec<Info> + Send + Sync> =
        Arc::new(|| vec![instructions_info("docs", Some("Use for product documentation"))]);
    let list: InstructionList = vec![reference_guidance_source(Arc::clone(&infos))];

    // Read: the described reference is a Value blob.
    let observed = instruction_read(&list);
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].0, "core/reference-guidance");
    let ReadOutcome::Value(blob) = &observed[0].1 else {
        panic!("expected a value outcome");
    };
    let values: BTreeMap<String, serde_json::Value> =
        std::iter::once(("core/reference-guidance".to_owned(), blob.clone())).collect();
    let initial = render_initial(&list, &values);
    assert!(initial.contains("<name>docs</name>"));
    assert!(initial.contains("<description>Use for product documentation</description>"));

    // Update: an added reference renders as a delta.
    let changed_infos: Arc<dyn Fn() -> Vec<Info> + Send + Sync> = Arc::new(|| {
        vec![
            instructions_info("docs", Some("Use for product documentation")),
            instructions_info("examples", Some("Use for examples")),
        ]
    });
    let changed_list: InstructionList = vec![reference_guidance_source(changed_infos)];
    let changed = instruction_read(&changed_list);
    let ReadOutcome::Value(next) = &changed[0].1 else {
        panic!("expected a value outcome");
    };
    let delta: BTreeMap<String, Option<serde_json::Value>> =
        std::iter::once(("core/reference-guidance".to_owned(), Some(next.clone()))).collect();
    let update = render_update(&changed_list, &values, &delta);
    assert!(update.starts_with(
        "New project references are available in addition to those previously listed:"
    ));

    // Removal: no described references is an observed absence.
    let empty: Arc<dyn Fn() -> Vec<Info> + Send + Sync> = Arc::new(Vec::new);
    let empty_list: InstructionList = vec![reference_guidance_source(empty)];
    let removed = instruction_read(&empty_list);
    assert_eq!(removed[0].1, ReadOutcome::Removed);
    let removal_delta: BTreeMap<String, Option<serde_json::Value>> =
        std::iter::once(("core/reference-guidance".to_owned(), None)).collect();
    let removal = render_update(&empty_list, &values, &removal_delta);
    assert_eq!(removal, super::instructions::REMOVED_TEXT);
}

// ---------------------------------------------------------------------------
// F-5: `reference.updated` on mutation (reference.ts:104-108)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reference_mutation_publishes_reference_updated() {
    let database = crate::database::Database::open_in_memory().await.expect("database");
    let bus = std::sync::Arc::new(crate::bus::Bus::new(
        database,
        crate::bus::BusOptions::deterministic(),
    ));
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let sink = std::sync::Arc::clone(&seen);
    let _guard = bus.listen(std::sync::Arc::new(move |event: &crate::bus::Event| {
        sink.lock().unwrap().push(event.type_.clone());
        Ok(())
    }));

    let mut service = ReferenceService::with_ensure(
        PathBuf::from("/repos"),
        std::sync::Arc::new(|_| {
            Err(CacheError::CloneFailed {
                repository: "owner/repo".to_string(),
                message: "test".to_string(),
            })
        }),
    );
    service.set_refresh_on_mutation(false);
    let service = std::sync::Arc::new(service);
    service.set_bus(std::sync::Arc::clone(&bus));

    service.add(
        "docs",
        Source::Local(LocalSource {
            path: PathBuf::from("/docs"),
            description: None,
            hidden: None,
        }),
    );
    service.remove("docs");
    // The publish is spawned on the current runtime; drive it to completion.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen, vec!["reference.updated".to_string(), "reference.updated".to_string()]);
}

/// G-198 — capture one git invocation's stdout for the shallow-clone
/// assertions.
fn git_output(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(path).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// G-198 — ref git.ts:246-258: cache clones are shallow with
/// `--depth (depth ?? 100)`; the depth input is the override seam.
#[test]
fn g198_cache_clones_shallow_with_default_depth_100() {
    // 120 commits on origin: the default depth 100 must cap the clone.
    let fixture = with_remote();
    for i in 0..119 {
        commit(&fixture.source, &format!("bulk {i}\n"), "bulk");
    }
    let (cache, _) = daily(&fixture);
    let result = cache
        .ensure(&EnsureInput {
            reference: fixture.reference.clone(),
            branch: None,
            refresh: Refresh::IfMissing,
            depth: None,
        })
        .unwrap();
    assert_eq!(result.status, "cloned");
    assert_eq!(git_output(&result.local_path, &["rev-parse", "--is-shallow-repository"]), "true");
    assert_eq!(git_output(&result.local_path, &["rev-list", "--count", "HEAD"]), "100");

    // The override seam: depth 1 caps the visible history.
    let fixture2 = with_remote();
    let (cache1, _) = daily(&fixture2);
    let one = cache1
        .ensure(&EnsureInput {
            reference: fixture2.reference.clone(),
            branch: None,
            refresh: Refresh::IfMissing,
            depth: Some(1),
        })
        .unwrap();
    assert_eq!(git_output(&one.local_path, &["rev-list", "--count", "HEAD"]), "1");
}

// ===========================================================================
// G-231 — ref fetch semantics: `--all --prune` + forced branch refspec
// (git.ts:269-287)
// ===========================================================================

/// (a) `--all`: a SECOND remote's branch refreshes on the cached repo's
/// update (the old plain `fetch origin` never touched it).
#[test]
fn g231_fetch_all_refreshes_second_remote() {
    let fixture = with_remote();
    let (cache, _) = daily(&fixture);
    let input = ensure(&fixture.reference);
    let initial = cache.ensure(&input).unwrap();

    // A second remote with its own branch.
    let second_origin = fixture.root.join("second.git");
    let second_source = fixture.root.join("second-source");
    std::fs::create_dir_all(&second_source).unwrap();
    git(&fixture.root, &["init", "--bare", second_origin.to_str().unwrap()]);
    git(&second_source, &["init"]);
    git(&second_source, &["config", "user.email", "test@example.com"]);
    git(&second_source, &["config", "user.name", "Test"]);
    std::fs::write(second_source.join("SECOND.md"), "v1\n").unwrap();
    git(&second_source, &["add", "."]);
    git(&second_source, &["commit", "-m", "second initial"]);
    git(&second_source, &["branch", "-M", "main"]);
    git(&second_source, &["remote", "add", "origin", &origin_url(&second_origin)]);
    git(&second_source, &["push", "-u", "origin", "main"]);
    git(&initial.local_path, &["remote", "add", "second", &origin_url(&second_origin)]);

    // The second remote advances AFTER the clone (no second remote was
    // known at clone time).
    std::fs::write(second_source.join("SECOND.md"), "v2\n").unwrap();
    git(&second_source, &["add", "."]);
    git(&second_source, &["commit", "-m", "second advance"]);
    git(&second_source, &["push", "origin", "main"]);

    let refreshed = cache
        .ensure(&EnsureInput { refresh: Refresh::Forced, ..ensure(&fixture.reference) })
        .unwrap();
    assert_eq!(refreshed.status, "refreshed");
    let tracked = git_output(&refreshed.local_path, &["rev-parse", "refs/remotes/second/main"]);
    let upstream = git_output(&second_source, &["rev-parse", "refs/heads/main"]);
    assert_eq!(tracked, upstream, "second remote refreshed via --all");
}

/// (b) `--prune`: a deleted upstream branch leaves remote-tracking.
#[test]
fn g231_fetch_prune_drops_deleted_upstream_branch() {
    let fixture = with_remote();
    git(&fixture.source, &["push", "origin", "main:refs/heads/feature"]);
    let (cache, _) = daily(&fixture);
    let input = ensure(&fixture.reference);
    let initial = cache.ensure(&input).unwrap();
    // A shallow clone tracks only the default branch (`--depth` implies
    // single-branch); widen to the standard glob refspec and populate
    // the tracking ref so there is something to prune.
    git(&initial.local_path, &["remote", "set-branches", "origin", "*"]);
    git(&initial.local_path, &["fetch", "origin"]);
    assert!(
        git_output(&initial.local_path, &["rev-parse", "--verify", "refs/remotes/origin/feature"])
            .len()
            == 40,
        "tracking ref present after fetch"
    );

    // Upstream deletes the branch; the refresh prunes the tracking ref.
    git(&fixture.source, &["push", "origin", "--delete", "feature"]);
    let refreshed = cache
        .ensure(&EnsureInput { refresh: Refresh::Forced, ..ensure(&fixture.reference) })
        .unwrap();
    assert_eq!(refreshed.status, "refreshed");
    let pruned = Command::new("git")
        .arg("-C")
        .arg(&refreshed.local_path)
        .args(["rev-parse", "--verify", "refs/remotes/origin/feature"])
        .output()
        .expect("git");
    assert!(!pruned.status.success(), "pruned tracking ref");
}

/// (c) The forced refspec: a non-fast-forward upstream rewrite lands in
/// refs/remotes/origin/<branch>.
#[test]
fn g231_forced_refspec_lands_non_fast_forward_update() {
    let fixture = with_remote();
    let (cache, _) = daily(&fixture);
    let branch_input = EnsureInput {
        reference: fixture.reference.clone(),
        branch: Some("main".to_owned()),
        refresh: Refresh::Forced,
        depth: None,
    };
    let initial = cache.ensure(&branch_input).unwrap();
    assert_eq!(read(&initial.local_path.join("README.md")), "one\n");

    // Rewrite history upstream (amend + force push): non-fast-forward
    // for the cached tracking ref.
    std::fs::write(fixture.source.join("README.md"), "rewritten\n").unwrap();
    git(&fixture.source, &["add", "."]);
    git(&fixture.source, &["commit", "--amend", "-m", "rewritten"]);
    git(&fixture.source, &["push", "--force", "origin", "main"]);

    let refreshed = cache.ensure(&branch_input).unwrap();
    assert_eq!(refreshed.status, "refreshed");
    let tracked = git_output(&refreshed.local_path, &["rev-parse", "refs/remotes/origin/main"]);
    let upstream = git_output(&fixture.source, &["rev-parse", "refs/heads/main"]);
    assert_eq!(tracked, upstream, "forced refspec updated the tracking ref");
    assert_eq!(read(&refreshed.local_path.join("README.md")), "rewritten\n");
}

// ---------------------------------------------------------------------------
// G-270② — the config-plugin transform (config/plugin/reference.ts:23-66)
// ---------------------------------------------------------------------------

fn config_document(path: Option<&str>, references: serde_json::Value) -> oc_schema::config::Entry {
    oc_schema::config::Entry::Document(Box::new(oc_schema::config::Document {
        info: serde_json::from_value(serde_json::json!({ "references": references }))
            .expect("config document"),
        path: path.map(str::to_owned),
    }))
}

#[test]
fn g270_sources_from_config_mirrors_the_config_plugin() {
    let location = Path::new("/work");
    let home = Path::new("/home/u");
    let entries = vec![
        config_document(
            Some("/work/opencode.json"),
            serde_json::json!({
                "local-abs": { "type": "local", "path": "/abs/docs" },
                "local-rel": "./docs",
                "local-home": "~/refs/x",
                "git-object": { "type": "git", "repository": "owner/repo", "branch": "main", "description": "D", "hidden": true },
                "git-shorthand": "owner/other",
                "git-bare-nonsense": "definitely a bad url!!",
                "bad/name": "./docs",
                "with space": "./docs",
                "": "./docs"
            }),
        ),
        // A path-less document (the global config has no file path):
        // relative paths resolve against the LOCATION directory.
        config_document(None, serde_json::json!({ "global-ref": "./global" })),
    ];
    let sources = ReferenceService::sources_from_config(&entries, location, home);
    let by_name = |name: &str| {
        sources
            .iter()
            .find(|(existing, _)| existing == name)
            .unwrap_or_else(|| panic!("{name} missing: {sources:?}"))
            .1
            .clone()
    };

    // Absolute local path stays.
    assert_eq!(
        by_name("local-abs"),
        Source::Local(LocalSource {
            path: PathBuf::from("/abs/docs"),
            description: None,
            hidden: None
        })
    );
    // Relative resolves against the DOCUMENT's directory (/work).
    assert_eq!(
        by_name("local-rel"),
        Source::Local(LocalSource {
            path: PathBuf::from("/work/docs"),
            description: None,
            hidden: None
        })
    );
    // `~` resolves against home.
    assert_eq!(
        by_name("local-home"),
        Source::Local(LocalSource {
            path: PathBuf::from("/home/u/refs/x"),
            description: None,
            hidden: None
        })
    );
    // Path-less document: relative resolves against the location.
    assert_eq!(
        by_name("global-ref"),
        Source::Local(LocalSource {
            path: PathBuf::from("/work/global"),
            description: None,
            hidden: None
        })
    );
    // Object git entry keeps everything.
    assert_eq!(
        by_name("git-object"),
        Source::Git(GitSource {
            repository: "owner/repo".to_owned(),
            branch: Some("main".to_owned()),
            description: Some("D".to_owned()),
            hidden: Some(true),
        })
    );
    // A bare string without ./, /, ~ is a git shorthand.
    assert_eq!(
        by_name("git-shorthand"),
        Source::Git(GitSource {
            repository: "owner/other".to_owned(),
            branch: None,
            description: None,
            hidden: None,
        })
    );
    // A bare string WITHOUT ./, /, ~ is a git shorthand however
    // unparsable it looks — classification happens before validation
    // (list() drops it later).
    assert_eq!(
        by_name("git-bare-nonsense"),
        Source::Git(GitSource {
            repository: "definitely a bad url!!".to_owned(),
            branch: None,
            description: None,
            hidden: None,
        })
    );

    // invalidAlias names are skipped.
    for skipped in ["bad/name", "with space", ""] {
        assert!(
            !sources.iter().any(|(name, _)| name == skipped),
            "{skipped} must be skipped: {sources:?}"
        );
    }
}

#[test]
fn g270_install_seeds_silently() {
    let fixture = with_remote();
    let repos = fixture.root.join("repos");
    std::fs::create_dir_all(&repos).unwrap();
    let (cache, _refresh_store) = daily(&fixture);
    let service = service(&repos, cache);

    // install(): bulk seed, no notify (no refresh spawned, entries land
    // alongside each other in one shot).
    service.install(vec![
        (
            "seeded".to_owned(),
            Source::Local(LocalSource {
                path: PathBuf::from("/docs"),
                description: None,
                hidden: None,
            }),
        ),
        (
            "second".to_owned(),
            Source::Local(LocalSource {
                path: PathBuf::from("/two"),
                description: None,
                hidden: None,
            }),
        ),
    ]);
    assert_eq!(service.sources().len(), 2, "bulk seed landed both entries");
    assert_eq!(service.list().len(), 2);

    // A same-name install REPLACES (insert-or-replace editor semantics).
    service.install(vec![(
        "seeded".to_owned(),
        Source::Local(LocalSource {
            path: PathBuf::from("/replaced"),
            description: Some("replaced".to_owned()),
            hidden: None,
        }),
    )]);
    assert_eq!(service.sources().len(), 2, "no duplicate row");
    let seeded = service.get("seeded").expect("seeded");
    assert_eq!(
        seeded,
        Source::Local(LocalSource {
            path: PathBuf::from("/replaced"),
            description: Some("replaced".to_owned()),
            hidden: None,
        })
    );
}

/// apply() = install + ONE notify: with refresh-on-mutation enabled, a
/// single apply of N git sources results in exactly one forked refresh
/// pass (the single-transform shape of the ref's config plugin — not one
/// per entry).
#[tokio::test]
async fn g270_apply_notifies_once_per_transform() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let passes = Arc::new(AtomicUsize::new(0));
    let ensure_passes = Arc::clone(&passes);
    let ensure: super::EnsureFn = Arc::new(move |_input| {
        ensure_passes.fetch_add(1, Ordering::SeqCst);
        Err(CacheError::InvalidBranch { branch: "x".to_owned(), message: "stop".to_owned() })
    });
    let service = Arc::new(ReferenceService::with_ensure(PathBuf::from("/repos"), ensure));

    service.apply(vec![
        (
            "one".to_owned(),
            Source::Git(GitSource {
                repository: "owner/one".to_owned(),
                branch: None,
                description: None,
                hidden: None,
            }),
        ),
        (
            "two".to_owned(),
            Source::Git(GitSource {
                repository: "owner/two".to_owned(),
                branch: None,
                description: None,
                hidden: None,
            }),
        ),
    ]);
    // The refresh is forked onto the runtime — give it a tick to land.
    for _ in 0..50 {
        if passes.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // ONE refresh pass over both sources = 2 ensure calls. The old
    // per-entry `add` shape would run pass(one) + pass(one,two) = 3.
    assert_eq!(passes.load(Ordering::SeqCst), 2, "exactly one refresh pass over both sources");
}
