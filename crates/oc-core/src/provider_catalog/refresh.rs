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

/// The production source (v2 `fetchApi`, models-dev.ts:345-352): a GET of
/// `<source>/api.json` with the app User-Agent and a 10-second timeout.
pub struct ReqwestCatalogSource {
    /// Remote base (v2 `defaultSource` when unset).
    pub url: String,
    /// `App.useragent` stamp.
    pub user_agent: String,
}

impl ReqwestCatalogSource {
    #[must_use]
    pub fn new(url: impl Into<String>, user_agent: impl Into<String>) -> Self {
        Self { url: url.into(), user_agent: user_agent.into() }
    }
}

#[async_trait]
impl CatalogSource for ReqwestCatalogSource {
    async fn fetch_text(&self) -> Result<String, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| format!("catalog http client: {error}"))?;
        let response = client
            .get(format!("{}/api.json", self.url.trim_end_matches('/')))
            .header("User-Agent", &self.user_agent)
            .send()
            .await
            .map_err(|error| format!("catalog fetch: {error}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("catalog fetch failed (HTTP {status})"));
        }
        response.text().await.map_err(|error| format!("catalog body: {error}"))
    }
}

/// The production KV cache (v2 `KV.Service`): one key holding
/// `{updatedAt, digest, body}` in the engine database.
pub struct DbCatalogKv {
    db: crate::database::Database,
}

impl DbCatalogKv {
    #[must_use]
    pub fn new(db: crate::database::Database) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CatalogKv for DbCatalogKv {
    async fn get(&self, key: &str) -> Option<String> {
        self.db.kv_get(key).await.ok().flatten().map(|value| value.to_string())
    }

    async fn set(&self, key: &str, value: &str) {
        let parsed: serde_json::Value =
            serde_json::from_str(value).unwrap_or(serde_json::Value::Null);
        let _ = self.db.kv_set(key, &parsed).await;
    }

    async fn remove(&self, key: &str) {
        let key = key.to_string();
        let _ = self
            .db
            .transaction(move |tx| {
                tx.kv_delete(&key)?;
                Ok(())
            })
            .await;
    }
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

    /// Boot-time populate from the KV cache (v2 `loadFromCache` +
    /// `populate`, models-dev.ts:312-321/358-375): a decodable cached body
    /// becomes the served catalog; a corrupt entry is removed. Returns
    /// whether a cached catalog was applied (the caller falls back to the
    /// snapshot otherwise — the frozen-snapshot floor).
    pub async fn populate_from_cache(&self) -> bool {
        let Some(cached) = self.kv.get(CACHE_KEY).await else { return false };
        let body = serde_json::from_str::<serde_json::Value>(&cached)
            .ok()
            .and_then(|entry| entry.get("body").and_then(|body| body.as_str()).map(str::to_string));
        let Some(body) = body else {
            self.kv.remove(CACHE_KEY).await;
            return false;
        };
        if let Ok(catalog) = Catalog::parse(&body) {
            *self.catalog.write() = Arc::new(catalog);
            return true;
        }
        self.kv.remove(CACHE_KEY).await;
        false
    }

    /// Serve an explicitly provided catalog (v2 `options.file` /
    /// `options.snapshot`): the embedder's boot catalog becomes the
    /// single source when no cache applied.
    pub fn adopt(&self, catalog: Catalog) {
        *self.catalog.write() = Arc::new(catalog);
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

    /// A tiny HTTP server speaking enough HTTP/1.1 for the reqwest source.
    async fn spawn_api_server(body: String) -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                let body = body.clone();
                tokio::spawn(async move {
                    let mut buffer = [0u8; 4096];
                    let _ = socket.read(&mut buffer).await;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}"), handle)
    }

    #[tokio::test]
    async fn provider_refresh_dbkv_roundtrip_and_boot_populate() {
        // The production KV over the engine database + the boot populate
        // (v2 loadFromCache): a cached body becomes the served catalog; a
        // corrupt entry is removed rather than trusted.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let db = crate::database::Database::open_file(dir.path().join("t.db")).await.expect("db");
        let kv = Arc::new(DbCatalogKv::new(db.clone()));

        kv.set("k", &serde_json::json!({ "a": 1 }).to_string()).await;
        assert_eq!(kv.get("k").await.as_deref(), Some(r#"{"a":1}"#));
        kv.remove("k").await;
        assert_eq!(kv.get("k").await, None);

        // Seed the cache exactly as a refresh writes it.
        let body = tiny_catalog();
        kv.set(
            CACHE_KEY,
            &serde_json::json!({
                "updatedAt": 1_000,
                "digest": "irrelevant",
                "body": body,
            })
            .to_string(),
        )
        .await;
        let service = CatalogService::new(
            FakeSource::new("unused") as Arc<dyn CatalogSource>,
            Arc::clone(&kv) as Arc<dyn CatalogKv>,
            Arc::new(SteppedClock(parking_lot::Mutex::new(60_000))) as Arc<dyn Clock>,
        );
        assert!(service.populate_from_cache().await, "cache applies");
        assert_eq!(service.get().provider_count(), 1, "cached body served");

        // Corrupt cache: removed, falls back (false) — the caller keeps
        // the frozen snapshot.
        kv.set(CACHE_KEY, "not-an-object").await;
        let service = CatalogService::new(
            FakeSource::new("unused") as Arc<dyn CatalogSource>,
            Arc::clone(&kv) as Arc<dyn CatalogKv>,
            Arc::new(SteppedClock(parking_lot::Mutex::new(60_000))) as Arc<dyn Clock>,
        );
        assert!(!service.populate_from_cache().await);
        assert_eq!(kv.get(CACHE_KEY).await, None, "corrupt entry removed");
        assert_eq!(service.get().provider_count(), 226, "snapshot floor holds");

        // `adopt` — the embedder-provided catalog becomes the source.
        service.adopt(Catalog::empty());
        assert_eq!(service.get().provider_count(), 0);
    }

    #[tokio::test]
    async fn provider_refresh_engine_boot_loop_updates_and_publishes() {
        // The v2 trigger (models-dev.ts:431-433): the boot-spawned loop
        // refreshes immediately — the live service catalog updates and
        // `models-dev.refreshed` publishes — while the engine's boot
        // snapshot (resolver plumbing) keeps serving until re-boot.
        let (url, server) = spawn_api_server(tiny_catalog()).await;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let engine = crate::engine::Engine::boot(crate::engine::EngineOptions {
            catalog_fetch: true,
            catalog_url: Some(url),
            ..crate::engine::EngineOptions::new(dir.path().join("data"), dir.path().join("home"))
        })
        .await;
        let mut subscription = engine.bus.subscribe();

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if engine.catalog_service.get().provider_count() == 1 {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "refresh loop never applied the fetched catalog"
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let frame = {
            use futures::StreamExt;
            tokio::time::timeout(
                deadline.saturating_duration_since(tokio::time::Instant::now()),
                subscription.next(),
            )
            .await
            .expect("timeout waiting for models-dev.refreshed")
            .expect("event")
        };
        assert_eq!(frame.type_, "models-dev.refreshed");
        assert_eq!(frame.data, serde_json::json!({}));
        server.abort();
    }

    #[tokio::test]
    async fn provider_refresh_offline_engine_stays_on_frozen_snapshot() {
        // v2 degradation: fetch failures are logged and ignored — the
        // catalog keeps serving the frozen snapshot (226 providers).
        let dir = tempfile::TempDir::new().expect("tempdir");
        let engine = crate::engine::Engine::boot(crate::engine::EngineOptions {
            catalog_fetch: true,
            // A closed local port: connection refused, fast.
            catalog_url: Some("http://127.0.0.1:9".to_string()),
            ..crate::engine::EngineOptions::new(dir.path().join("data"), dir.path().join("home"))
        })
        .await;
        assert!(engine.catalog_service.refresh(false).await.is_err(), "offline fetch fails");
        assert_eq!(engine.catalog_service.get().provider_count(), 226, "frozen snapshot");
        assert_eq!(engine.catalog.provider_count(), 226, "engine snapshot unchanged");
        assert_eq!(engine.catalog_service.version(), 0, "no invalidation");
    }
}
