//! Port of `file-mutation.ts` — serialized file mutations with
//! process-shared transaction locks and BOM-preserving text writes.
//!
//! - [`FileMutation::with_lock`] resolves + dedupes + sorts targets, then
//!   nests the (process-global) keyed locks **right-to-left**, so two
//!   transactions over overlapping target sets acquire in the same order
//!   and never deadlock.
//! - [`FileMutation::write`] serializes per absolute path (an
//!   uninterruptible per-target lock in v2).
//! - [`FileMutation::write_text_preserving_bom`] keeps an existing UTF-8
//!   BOM and emits at most one (bom.ts `split`/`join`/`has`).

//! Lint posture: ports of TS sources keep the mechanical-idiom allows the
//! runner established (numeric casts, exhaustive tables, ownership queries).
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::match_same_arms,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::option_if_let_else,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args,
    clippy::type_complexity,
    clippy::large_enum_variant,
    clippy::struct_field_names,
    clippy::unnested_or_patterns,
    clippy::single_match_else,
    clippy::len_without_is_empty,
    clippy::module_name_repetitions,
    clippy::result_large_err,
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::trivially_copy_pass_by_ref,
    clippy::significant_drop_tightening,
    clippy::large_futures,
    clippy::option_option,
    clippy::assert_is_empty,
    clippy::match_wildcard_for_single_variants,
    clippy::semicolon_if_nothing_returned,
    clippy::format_collect,
    clippy::default_trait_access,
    clippy::ignored_unit_patterns,
    clippy::duration_suboptimal_units,
    clippy::explicit_iter_loop,
    clippy::manual_let_else,
    clippy::unnecessary_wraps,
    clippy::ref_option,
    clippy::unused_async,
    clippy::needless_borrow,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::min_ident_chars,
    clippy::unreadable_literal,
    clippy::similar_names,
    clippy::unused_async_trait_impl,
    clippy::unused_self,
    clippy::doc_lazy_continuation
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::Mutex as AsyncMutex;

use crate::file_access;

/// BOM code point (bom.ts).
const BOM: &str = "\u{FEFF}";

/// `Bom.split` — split ALL leading BOMs off the text
/// (`text.replace(/^\uFEFF+/, "")`).
#[must_use]
pub fn bom_split(text: &str) -> (bool, &str) {
    let stripped = text.trim_start_matches(BOM);
    (stripped.len() != text.len(), stripped)
}

/// `Bom.join` — re-attach exactly one BOM when requested.
#[must_use]
pub fn bom_join(text: &str, bom: bool) -> String {
    let (_, stripped) = bom_split(text);
    if bom {
        format!("{BOM}{stripped}")
    } else {
        stripped.to_string()
    }
}

/// `Bom.has` — the UTF-8 BOM bytes at the head of `content`.
#[must_use]
pub fn bom_has(content: &[u8]) -> bool {
    content.len() >= 3 && content[0] == 0xef && content[1] == 0xbb && content[2] == 0xbf
}

/// One text file's decoded content plus its BOM flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextContent {
    pub text: String,
    pub bom: bool,
}

/// `FileMutation.readText` — decode bytes, splitting off a leading BOM.
pub fn read_text(absolute: &Path) -> std::io::Result<TextContent> {
    let bytes = std::fs::read(absolute)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let (bom, text) = bom_split(&text);
    let text = text.to_string();
    Ok(TextContent { text, bom })
}

/// `FileMutation.syncTextBom` — normalize the stored BOM to `bom`; returns
/// the text and whether a rewrite happened.
pub fn sync_text_bom(absolute: &Path, bom: bool) -> std::io::Result<TextContent> {
    let current = read_text(absolute)?;
    let canonical = bom_join(&current.text, bom);
    if canonical != String::from_utf8_lossy(&std::fs::read(absolute)?).into_owned() {
        std::fs::write(absolute, canonical.as_bytes())?;
    }
    Ok(TextContent { text: current.text, bom })
}

/// `FileMutation.Target` — the resolved absolute path + permission resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationTarget {
    pub absolute: PathBuf,
    pub resource: String,
}

/// `FileMutation.WriteResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteResult {
    pub operation: &'static str,
    pub target: PathBuf,
    pub resource: String,
    pub existed: bool,
}

/// The per-process keyed lock tables (file-mutation.ts keeps one table on
/// the layer and one global `transactionLocks` shared across Location
/// graphs; both are process-local here).
#[derive(Default)]
struct LockTables {
    transactions: DashMap<String, Arc<AsyncMutex<()>>>,
    targets: DashMap<String, Arc<AsyncMutex<()>>>,
}

/// Share transaction locks across Location graphs (module-level table).
static GLOBAL_TABLES: std::sync::OnceLock<LockTables> = std::sync::OnceLock::new();

fn tables() -> &'static LockTables {
    GLOBAL_TABLES.get_or_init(LockTables::default)
}

/// The guard returned by [`FileMutation::acquire_locks`] — holds the nested
/// transaction locks; dropping releases them in acquisition order.
pub struct TransactionGuard {
    guards: Vec<tokio::sync::OwnedMutexGuard<()>>,
}

impl TransactionGuard {
    /// Runs `body` while the locks are held (the future owns the guard).
    pub async fn scope<F, R>(mut self, body: F) -> R
    where
        F: std::future::Future<Output = R>,
    {
        let outcome = body.await;
        // Drop in acquisition (left-to-right) order, then the rest.
        while let Some(guard) = self.guards.pop() {
            drop(guard);
        }
        outcome
    }
}

/// The FileMutation service (one per Location; the transaction table is
/// process-global).
#[derive(Debug, Default)]
pub struct FileMutation {
    location_directory: PathBuf,
    home: PathBuf,
}

impl FileMutation {
    /// Service bound to a Location directory (for `resolvePath`).
    #[must_use]
    pub fn new(location_directory: PathBuf, home: PathBuf) -> Self {
        Self { location_directory, home }
    }

    /// Resolve an input path against the location (file-access.ts
    /// `resolvePath`).
    #[must_use]
    pub fn resolve_path(&self, input: &str) -> PathBuf {
        file_access::resolve_path(&self.location_directory, input, &self.home)
    }

    /// `withLock` — resolve + dedupe + sort, then acquire the global keyed
    /// locks right-to-left (the sorted order makes nesting deadlock-free).
    pub async fn acquire_locks(&self, targets: &[String]) -> TransactionGuard {
        let resolved: BTreeSet<String> =
            targets.iter().map(|t| self.resolve_path(t).to_string_lossy().into_owned()).collect();
        let mut guards = Vec::new();
        for target in resolved.iter().rev() {
            let lock = tables()
                .transactions
                .entry(target.clone())
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone();
            guards.push(lock.lock_owned().await);
        }
        TransactionGuard { guards }
    }

    /// `withLock` as a direct future wrapper.
    pub async fn with_lock<F>(
        &self,
        targets: &[String],
        body: F,
    ) -> <F as std::future::Future>::Output
    where
        F: std::future::Future,
    {
        let guard = self.acquire_locks(targets).await;
        guard.scope(body).await
    }

    async fn lock_target(&self, absolute: &Path) -> Arc<AsyncMutex<()>> {
        let key = absolute.to_string_lossy().into_owned();
        tables().targets.entry(key).or_insert_with(|| Arc::new(AsyncMutex::new(()))).clone()
    }

    /// `write` — serialized per-target write of raw content.
    pub async fn write(
        &self,
        target: &MutationTarget,
        content: Vec<u8>,
    ) -> std::io::Result<WriteResult> {
        let lock = self.lock_target(&target.absolute).await;
        let _guard = lock.lock().await;
        let existed = target.absolute.symlink_metadata().is_ok();
        if let Some(parent) = target.absolute.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target.absolute, content)?;
        Ok(WriteResult {
            operation: "write",
            target: target.absolute.clone(),
            resource: target.resource.clone(),
            existed,
        })
    }

    /// `writeTextPreservingBom` — keep an existing BOM, emit at most one.
    pub async fn write_text_preserving_bom(
        &self,
        target: &MutationTarget,
        content: &str,
    ) -> std::io::Result<WriteResult> {
        let lock = self.lock_target(&target.absolute).await;
        let _guard = lock.lock().await;
        let (next_bom, next_text) = bom_split(content);
        let current = std::fs::read(&target.absolute).ok();
        let existed = current.is_some();
        let keep_bom = current.as_deref().is_some_and(bom_has) || next_bom;
        if let Some(parent) = target.absolute.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target.absolute, bom_join(next_text, keep_bom).as_bytes())?;
        Ok(WriteResult {
            operation: "write",
            target: target.absolute.clone(),
            resource: target.resource.clone(),
            existed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mutation(dir: &Path) -> FileMutation {
        FileMutation::new(dir.to_path_buf(), dir.to_path_buf())
    }

    fn target(dir: &Path, resource: &str) -> MutationTarget {
        MutationTarget { absolute: dir.join(resource), resource: resource.to_string() }
    }

    #[tokio::test]
    async fn file_mutation_writes_existing_internal_file_with_stable_result() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        std::fs::write(dir.path().join("hello.txt"), "before").expect("write");
        let service = mutation(dir.path());
        let result = service
            .write(&target(dir.path(), "hello.txt"), b"after".to_vec())
            .await
            .expect("write");
        assert_eq!(
            result,
            WriteResult {
                operation: "write",
                target: dir.path().join("hello.txt"),
                resource: "hello.txt".into(),
                existed: true,
            }
        );
        assert_eq!(std::fs::read_to_string(dir.path().join("hello.txt")).unwrap(), "after");
    }

    #[tokio::test]
    async fn file_mutation_creates_parent_directories() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let service = mutation(dir.path());
        let result = service
            .write(&target(dir.path(), "src/nested/hello.txt"), b"hello".to_vec())
            .await
            .expect("write");
        assert!(!result.existed);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/nested/hello.txt")).unwrap(),
            "hello"
        );
    }

    #[tokio::test]
    async fn file_mutation_preserves_exactly_one_bom() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let service = mutation(dir.path());
        std::fs::write(dir.path().join("preserved.txt"), "\u{FEFF}before").expect("write");
        service
            .write_text_preserving_bom(&target(dir.path(), "preserved.txt"), "\u{FEFF}after")
            .await
            .expect("preserve");
        service
            .write_text_preserving_bom(
                &target(dir.path(), "created.txt"),
                "\u{FEFF}\u{FEFF}\u{FEFF}created",
            )
            .await
            .expect("create");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("preserved.txt")).unwrap(),
            "\u{FEFF}after"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("created.txt")).unwrap(),
            "\u{FEFF}created"
        );
    }

    #[tokio::test]
    async fn file_mutation_serializes_concurrent_writes_to_the_same_target() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        std::fs::write(dir.path().join("shared.txt"), "initial").expect("write");
        let service = Arc::new(mutation(dir.path()));
        let first = {
            let service = Arc::clone(&service);
            let target = target(dir.path(), "shared.txt");
            tokio::spawn(async move { service.write(&target, b"first".to_vec()).await })
        };
        let second = {
            let service = Arc::clone(&service);
            let target = target(dir.path(), "shared.txt");
            tokio::spawn(async move { service.write(&target, b"second".to_vec()).await })
        };
        first.await.expect("join").expect("write");
        second.await.expect("join").expect("write");
        let content = std::fs::read_to_string(dir.path().join("shared.txt")).unwrap();
        assert!(content == "first" || content == "second", "one whole write wins: {content}");
    }

    #[tokio::test]
    async fn file_mutation_transaction_locks_exclude_concurrent_holders() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let first_service = mutation(dir.path());
        let second_service = mutation(dir.path());
        let shared = dir.path().join("shared.txt").to_string_lossy().into_owned();

        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let first = {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            let targets = vec![shared.clone()];
            tokio::spawn(async move {
                first_service
                    .with_lock(&targets, async move {
                        started.notify_one();
                        release.notified().await;
                    })
                    .await
            })
        };
        started.notified().await;
        let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = {
            let entered = Arc::clone(&entered);
            let targets = vec![shared.clone()];
            tokio::spawn(async move {
                second_service
                    .with_lock(&targets, async move {
                        entered.store(true, std::sync::atomic::Ordering::SeqCst);
                    })
                    .await
            })
        };
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(!entered.load(std::sync::atomic::Ordering::SeqCst), "second waits for the lock");
        release.notify_one();
        first.await.expect("join first");
        probe.await.expect("join probe");
        assert!(entered.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn file_mutation_distinct_targets_proceed_independently() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let service = mutation(dir.path());
        let first_started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let first = {
            let started = Arc::clone(&first_started);
            let release = Arc::clone(&release);
            let targets = vec![dir.path().join("first.txt").to_string_lossy().into_owned()];
            tokio::spawn(async move {
                service
                    .with_lock(&targets, async move {
                        started.notify_one();
                        release.notified().await;
                    })
                    .await
            })
        };
        first_started.notified().await;
        // A transaction over a different target is not blocked.
        let second_finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = {
            let service = mutation(dir.path());
            let second_finished = Arc::clone(&second_finished);
            let targets = vec![dir.path().join("second.txt").to_string_lossy().into_owned()];
            tokio::spawn(async move {
                service
                    .with_lock(&targets, async move {
                        second_finished.store(true, std::sync::atomic::Ordering::SeqCst);
                    })
                    .await
            })
        };
        probe.await.expect("join probe");
        assert!(second_finished.load(std::sync::atomic::Ordering::SeqCst));
        release.notify_one();
        first.await.expect("join first");
    }

    #[test]
    fn file_mutation_bom_helpers() {
        assert_eq!(bom_split("\u{FEFF}x"), (true, "x"));
        assert_eq!(bom_split("\u{FEFF}\u{FEFF}x"), (true, "x"), "all leading BOMs split");
        assert_eq!(bom_split("x"), (false, "x"));
        assert_eq!(bom_split(""), (false, ""));
        assert_eq!(bom_join("\u{FEFF}x", false), "x");
        assert_eq!(bom_join("x", true), "\u{FEFF}x");
        assert!(bom_has(&[0xef, 0xbb, 0xbf, 0x00]));
        assert!(!bom_has(&[0xef, 0xbb]));
        assert!(!bom_has(b"abc"));
    }
}
