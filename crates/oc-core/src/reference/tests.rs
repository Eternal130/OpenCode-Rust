//! Ports of the v2 reference test suites: `repository.test.ts`,
//! `repository-cache.test.ts` (local fixture git repos — never network),
//! `reference.test.ts`, and `reference-instructions.test.ts`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::repository::{cache_identity, cache_path, is_file, is_remote, parse, parse_remote, same, validate_branch, Reference};
use super::repository_cache::{CacheError, EnsureInput, MemoryRefreshStore, Refresh, RefreshStore, RepositoryCache};
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
    assert!(matches!(
        unsupported,
        super::repository::RepositoryError::UnsupportedLocal(_)
    ));
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

    for input in ["..:repo", "git@..:repo", "../owner/repo", "ssh://../repo", "ssh://..:22/repo", "https://%2e%2e/owner/repo", ".:repo"] {
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
    git(root.path(), &["--git-dir", origin.to_str().unwrap(), "symbolic-ref", "HEAD", "refs/heads/main"]);
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

fn cache_layer(fixture: &Fixture, store: Arc<dyn RefreshStore>, now: SystemTime) -> RepositoryCache {
    std::fs::create_dir_all(fixture.root.join("repos")).unwrap();
    RepositoryCache::new(
        fixture.root.join("repos"),
        Arc::new(MutableClock(now)),
        store,
    )
}

fn daily(fixture: &Fixture) -> (RepositoryCache, Arc<MemoryRefreshStore>) {
    let store = Arc::new(MemoryRefreshStore::default());
    (cache_layer(fixture, Arc::clone(&store) as Arc<dyn RefreshStore>, SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)), store)
}

fn ensure(reference: &super::repository::RemoteReference) -> EnsureInput {
    EnsureInput { reference: reference.clone(), branch: None, refresh: Refresh::Daily }
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
    let cache = cache_layer(&fixture, Arc::clone(&store) as Arc<dyn RefreshStore>, SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_100));
    assert_eq!(cache.ensure(&ensure(&fixture.reference)).unwrap().status, "cached");
    let key = format!("repository-cache:{}", initial.local_path.to_string_lossy());
    store.set_attempted_at(&key, 1_700_000_100_000 - 86_400_000);

    let results: Vec<_> = [0, 1]
        .iter()
        .map(|_| cache.ensure(&ensure(&fixture.reference)).unwrap().status)
        .collect();
    assert!(results.contains(&"refreshed"));
    assert!(results.contains(&"cached"));
    assert_eq!(read(&initial.local_path.join("README.md")), "two\n");

    // A missing checkout is recreated even with a fresh timestamp.
    std::fs::remove_dir_all(&initial.local_path).unwrap();
    let store2 = Arc::new(MemoryRefreshStore::default());
    store2.set_attempted_at(&key, 1_700_000_100_000);
    let cache = cache_layer(&fixture, store2, SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_100));
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
        cache.ensure(&EnsureInput { refresh: Refresh::Forced, ..ensure(&fixture.reference) }).unwrap().status,
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
    let source = Source::Local(LocalSource { path: PathBuf::from("/docs"), description: None, hidden: None });
    references.add("docs", source.clone());
    assert_eq!(references.get("docs"), Some(source.clone()));
    assert_eq!(references.get("missing"), None);
    let replacement = Source::Git(GitSource { repository: "owner/repo".to_owned(), branch: None, description: None, hidden: None });
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
        Source::Local(LocalSource { path: PathBuf::from("/docs"), description: None, hidden: None }),
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
    references.add("invalid", Source::Git(GitSource { repository: "invalid".to_owned(), branch: None, description: None, hidden: None }));
    references.add(
        "invalid-branch",
        Source::Git(GitSource { repository: "owner/repo".to_owned(), branch: Some("../escape".to_owned()), description: None, hidden: None }),
    );
    references.add(
        "file",
        Source::Git(GitSource { repository: "file:///docs".to_owned(), branch: None, description: None, hidden: None }),
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
    let operations: Arc<PLMutex<Vec<(String, Option<String>, Refresh)>>> = Arc::new(PLMutex::new(Vec::new()));
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
                local_path: cache_path(&repos, &Reference::Remote(input.reference.clone()), input.branch.as_deref()),
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
        Source::Local(LocalSource { path: PathBuf::from("/docs"), description: None, hidden: None }),
    );
    references.add(
        "invalid",
        Source::Git(GitSource { repository: "invalid".to_owned(), branch: None, description: None, hidden: None }),
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
        [("https://github.com/owner/repo.git".to_owned(), Some("feature/docs".to_owned()), Refresh::Daily)]
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
        instruction_read, reference_guidance_source, render_initial, render_update, InstructionList,
        ReadOutcome,
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
