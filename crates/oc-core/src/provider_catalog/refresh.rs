//! Remote catalog refresh — KV-cached fetch of
//! `https://models.opencode.ai/api.json` behind traits (port of the fetch
//! layer of `packages/core/src/models-dev.ts`).
//!
//! - Offline default: the embedded snapshot ([`Catalog::embedded`]) — no
//!   network access happens implicitly; tests never fetch.
//! - [`CatalogSource`] fetches the body (oc-core has no HTTP client; the
//!   server lane provides the reqwest-backed impl — `TODO(server-lane)`).
//! - [`CatalogKv`] is the durable cache (`models-dev:catalog`).
//! - [`CatalogService`] glues them: TTL-gated refresh, body-digest skip for
//!   byte-identical bodies, invalidation + version bump.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use oc_util::determinism::Clock;
use parking_lot::RwLock;

use crate::provider_catalog::catalog::{body_digest, Catalog};

/// Default remote source (v2 `defaultSource`).
pub const DEFAULT_SOURCE: &str = "https://models.opencode.ai";

/// KV cache key for the default source (v2 `cacheKey`).
pub const CACHE_KEY: &str = "models-dev:catalog";

/// Refresh poll TTL (v2: 5 minutes).
pub const REFRESH_TTL: Duration = Duration::from_secs(300);

/// Remote catalog fetcher. The concrete HTTP implementation lives in the
/// server lane (`TODO(server-lane)`); tests inject fakes.
#[async_trait]
pub trait CatalogSource: Send + Sync {
    /// Fetch the raw `api.json` body.
    async fn fetch_text(&self) -> Result<String, String>;
}

/// Durable KV cache for the catalog body (v2 `KV.Service` usage: one key
/// holding `{updatedAt, digest, body}`).
///
/// `TODO(plugin-lane)` shares this seam with the plugin storage KV.
#[async_trait]
pub trait CatalogKv: Send + Sync {
    async fn get(&self, key: &str) -> Option<String>;
    async fn set(&self, key: &str, value: &str);
    async fn remove(&self, key: &str);
}

/// What a [`CatalogService::refresh`] pass did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// Nothing to do: cache fresh within the TTL.
    SkippedFresh,
    /// Body digest unchanged — no invalidation, no event (v2 keeps
    /// downstream listeners quiet).
    SkippedDigest,
    /// Fetched, decoded, cached, invalidated.
    Updated,
}

/// Catalog with refresh wiring. `get()` returns the current catalog
/// (embedded snapshot until the first successful refresh).
pub struct CatalogService {
    catalog: RwLock<Arc<Catalog>>,
    version: std::sync::atomic::AtomicU64,
    source: Arc<dyn CatalogSource>,
    kv: Arc<dyn CatalogKv>,
    clock: Arc<dyn Clock>,
    ttl: Duration,
    /// Optional bus: publishes `models-dev.refreshed` after every
    /// `RefreshOutcome::Updated` (v2 models-dev.ts:416-422 — the refresh
    /// loop's digest-change notification; `SkipDigest` stays silent).
    bus: Option<crate::bus::Bus>,
}

impl CatalogService {
    /// Service over a source + KV cache with an injectable clock.
    pub fn new(
        source: Arc<dyn CatalogSource>,
        kv: Arc<dyn CatalogKv>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            catalog: RwLock::new(Arc::new(Catalog::embedded().clone())),
            version: std::sync::atomic::AtomicU64::new(0),
            source,
            kv,
            clock,
            ttl: REFRESH_TTL,
            bus: None,
        }
    }

    /// Override the refresh TTL (tests).
    #[must_use]
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Attach the bus for `models-dev.refreshed` publication
    /// (`models-dev.ts:422` — published globally, empty payload, only
    /// after a digest-changing refresh).
    #[must_use]
    pub fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Current catalog.
    #[must_use]
    pub fn get(&self) -> Arc<Catalog> {
        Arc::clone(&self.catalog.read())
    }

    /// Invalidation counter (bumps on every [`RefreshOutcome::Updated`]).
    pub fn version(&self) -> u64 {
        self.version.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// One refresh pass (v2 `refresh(force)`): TTL gate → fetch → digest
    /// gate → decode → cache write → invalidate. Errors are returned, not
    /// swallowed (v2 logs-and-ignores; the caller decides).
    pub async fn refresh(&self, force: bool) -> Result<RefreshOutcome, String> {
        let cached = self.kv.get(CACHE_KEY).await;
        if let Some(cached) = &cached {
            let updated_at = cached_updated_at(cached).unwrap_or(0);
            let age = self.clock.unix_ms() - updated_at;
            let ttl_ms = i64::try_from(self.ttl.as_millis()).unwrap_or(i64::MAX);
            if !force && age < ttl_ms {
                return Ok(RefreshOutcome::SkippedFresh);
            }
        }
        let text = self.source.fetch_text().await?;
        let digest = body_digest(&text);
        if !force && cached.as_deref().and_then(cached_digest).as_deref() == Some(digest.as_str()) {
            return Ok(RefreshOutcome::SkippedDigest);
        }
        // decode before writing (v2 decodes to validate before caching)
        let catalog = Catalog::parse(&text)?;
        let entry = serde_json::json!({
            "updatedAt": self.clock.unix_ms(),
            "digest": digest,
            "body": text,
        });
        self.kv.set(CACHE_KEY, &entry.to_string()).await;
        *self.catalog.write() = Arc::new(catalog);
        self.version.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // v2 publishes `ModelsDev.Event.Refreshed` ({}) after a
        // digest-changing refresh (models-dev.ts:416-422) — best-effort:
        // a failed notification never fails the refresh.
        if let Some(bus) = &self.bus {
            if let Some(def) =
                crate::bus::registry::by_type(crate::bus::registry::MODELS_DEV_REFRESHED)
            {
                let options = crate::bus::PublishOptions { global: true, ..Default::default() };
                let _ = bus.publish(def, serde_json::json!({}), options).await;
            }
        }
        Ok(RefreshOutcome::Updated)
    }
}

fn cached_updated_at(entry: &str) -> Option<i64> {
    serde_json::from_str::<serde_json::Value>(entry).ok()?.get("updatedAt")?.as_i64()
}

fn cached_digest(entry: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(entry)
        .ok()?
        .get("digest")?
        .as_str()
        .map(str::to_string)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct FakeSource {
        body: RwLock<String>,
        fetches: AtomicU64,
    }

    impl FakeSource {
        fn new(body: &str) -> Arc<Self> {
            Arc::new(Self { body: RwLock::new(body.to_string()), fetches: AtomicU64::new(0) })
        }
    }

    #[async_trait]
    impl CatalogSource for FakeSource {
        async fn fetch_text(&self) -> Result<String, String> {
            self.fetches.fetch_add(1, Ordering::SeqCst);
            Ok(self.body.read().clone())
        }
    }

    #[derive(Default)]
    struct MemoryKv {
        map: RwLock<std::collections::BTreeMap<String, String>>,
    }

    #[async_trait]
    impl CatalogKv for MemoryKv {
        async fn get(&self, key: &str) -> Option<String> {
            self.map.read().get(key).cloned()
        }
        async fn set(&self, key: &str, value: &str) {
            self.map.write().insert(key.to_string(), value.to_string());
        }
        async fn remove(&self, key: &str) {
            self.map.write().remove(key);
        }
    }

    struct SteppedClock(parking_lot::Mutex<i64>);

    #[allow(clippy::cast_sign_loss, reason = "test clock ms is always positive")]
    impl Clock for SteppedClock {
        fn now(&self) -> std::time::SystemTime {
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(self.0.lock().max(0) as u64)
        }
    }

    fn tiny_catalog() -> String {
        serde_json::json!({
            "tiny": {
                "id": "tiny",
                "name": "Tiny",
                "env": ["TINY_API_KEY"],
                "npm": "@ai-sdk/openai-compatible",
                "api": "https://api.tiny.test/v1",
                "models": {
                    "tiny-1": {
                        "id": "tiny-1",
                        "name": "Tiny One",
                        "release_date": "2026-01-01",
                        "attachment": false,
                        "reasoning": false,
                        "tool_call": true,
                        "limit": {"context": 8192, "output": 1024}
                    }
                }
            }
        })
        .to_string()
    }

    #[tokio::test]
    async fn provider_refresh_publishes_models_dev_refreshed_after_update() {
        // v2 models-dev.ts:416-422 — a digest-changing refresh publishes
        // `models-dev.refreshed` ({}); a byte-identical body stays silent.
        use futures::StreamExt;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let db = crate::database::Database::open_file(dir.path().join("t.db")).await.expect("db");
        let bus = crate::bus::Bus::new(db, crate::bus::BusOptions::deterministic());
        let mut subscription = bus.subscribe();
        let source = FakeSource::new(&tiny_catalog());
        let kv = Arc::new(MemoryKv::default());
        let clock = Arc::new(SteppedClock(parking_lot::Mutex::new(1_000)));
        let service = CatalogService::new(
            Arc::clone(&source) as Arc<dyn CatalogSource>,
            Arc::clone(&kv) as Arc<dyn CatalogKv>,
            Arc::clone(&clock) as Arc<dyn Clock>,
        )
        .with_ttl(Duration::from_millis(50))
        .with_bus(bus);

        assert_eq!(service.refresh(false).await.unwrap(), RefreshOutcome::Updated);
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), subscription.next())
            .await
            .expect("timeout")
            .expect("event");
        assert_eq!(frame.type_, "models-dev.refreshed");
        assert_eq!(frame.data, serde_json::json!({}));

        // Past the TTL with an identical body: digest skip, no second
        // event (force would bypass the digest gate by design).
        *clock.0.lock() += 1_000;
        assert_eq!(service.refresh(false).await.unwrap(), RefreshOutcome::SkippedDigest);
        let none =
            tokio::time::timeout(std::time::Duration::from_millis(50), subscription.next()).await;
        assert!(none.is_err(), "no event on digest skip");
    }

    #[tokio::test]
    async fn provider_refresh_fetches_caches_and_invalidates() {
        let source = FakeSource::new(&tiny_catalog());
        let kv = Arc::new(MemoryKv::default());
        let clock = Arc::new(SteppedClock(parking_lot::Mutex::new(1_000)));
        let service = CatalogService::new(
            Arc::clone(&source) as Arc<dyn CatalogSource>,
            Arc::clone(&kv) as Arc<dyn CatalogKv>,
            Arc::clone(&clock) as Arc<dyn Clock>,
        );

        assert_eq!(service.get().provider_count(), 226, "starts on the embedded snapshot");
        assert_eq!(
            service.refresh(false).await.unwrap(),
            RefreshOutcome::Updated,
            "first refresh fetches (no cache)"
        );
        assert_eq!(source.fetches.load(Ordering::SeqCst), 1);
        assert!(kv.get(CACHE_KEY).await.is_some(), "cache written");
        assert_eq!(service.get().provider_count(), 1, "invalidated onto the fetched body");
        assert_eq!(service.version(), 1);

        // within TTL: skipped without fetching
        assert_eq!(service.refresh(false).await.unwrap(), RefreshOutcome::SkippedFresh);
        assert_eq!(source.fetches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn provider_refresh_skips_byte_identical_bodies() {
        let source = FakeSource::new(&tiny_catalog());
        let kv = Arc::new(MemoryKv::default());
        let clock = Arc::new(SteppedClock(parking_lot::Mutex::new(1_000)));
        let service = CatalogService::new(
            Arc::clone(&source) as Arc<dyn CatalogSource>,
            Arc::clone(&kv) as Arc<dyn CatalogKv>,
            Arc::clone(&clock) as Arc<dyn Clock>,
        )
        .with_ttl(Duration::from_millis(50));
        service.refresh(false).await.unwrap();

        // past the TTL with an identical body: digest skip, no invalidation
        *clock.0.lock() += 1_000;
        let version = service.version();
        assert_eq!(service.refresh(false).await.unwrap(), RefreshOutcome::SkippedDigest);
        assert_eq!(service.version(), version, "no invalidation for identical body");

        // force ignores both gates
        assert_eq!(service.refresh(true).await.unwrap(), RefreshOutcome::Updated);
    }

    #[tokio::test]
    async fn provider_refresh_rejects_invalid_bodies_without_caching() {
        let source = FakeSource::new("not json");
        let kv = Arc::new(MemoryKv::default());
        let clock = Arc::new(SteppedClock(parking_lot::Mutex::new(1_000)));
        let service = CatalogService::new(
            Arc::clone(&source) as Arc<dyn CatalogSource>,
            Arc::clone(&kv) as Arc<dyn CatalogKv>,
            Arc::clone(&clock) as Arc<dyn Clock>,
        );
        let error = service.refresh(false).await.unwrap_err();
        assert!(error.contains("invalid catalog"), "decode failure surfaces: {error}");
        assert!(kv.get(CACHE_KEY).await.is_none(), "bad body never cached");
    }
}
