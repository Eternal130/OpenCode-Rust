//! G-332 — plugin markdown code-block renderers
//! (`ctx.markdown.registerCodeBlockRenderer(language, render)`, ref
//! `packages/tui/src/plugin/api.tsx:168-178` + `plugin/markdown.ts`):
//! the render functions stay in the guest; this module owns the
//! process-wide dispatch TABLE the markdown pipeline consults and the
//! bounded content CACHE that keeps rendering native.
//!
//! * [`install_table`] — built by `plugin_boot` from the registration
//!   snapshot: raw languages normalize through
//!   [`opentui_core::tree_sitter::info_string_to_filetype`] and the
//!   LAST registration per normalized filetype wins (the ref's
//!   merge, `plugin/markdown.ts:5-18` — enable order, cross-plugin
//!   shadowing allowed, the builtin latex/mermaid arms included).
//! * [`lookup`] — the pipeline's consult (returns the owning plugin
//!   + RAW language + the table fingerprint).
//! * [`cached_rows`] / [`spawn_render`] — content-hash cache keyed
//!   `(filetype, content hash, table fingerprint, syntax hash)`. A
//!   MISS renders the default path this frame and fires a bounded
//!   off-loop `PluginHost::render_markdown_block`; the reply lands
//!   in the cache and bumps the repaint version (the pump calls
//!   [`rendered_version`] and requests a redraw). An ERROR/timeout
//!   caches permanently for that (code, fingerprint) pair — no
//!   per-frame retry storms (logged once).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use oc_plugin_host::PluginHost;

/// The bound on one plugin markdown render (the dialog/claim family).
const RENDER_TIMEOUT: Duration = Duration::from_secs(4);

/// The cache capacity before a full clear (the `styled_cached`
/// pattern — streaming chats churn keys; a clear never outlives
/// settled content).
const CACHE_CAPACITY: usize = 512;

/// One cached render outcome. `Err(())` marks a PERMANENT failure
/// (render error / timeout) for the (code, fingerprint) pair.
type CachedRows = Result<Vec<String>, ()>;

struct State {
    /// normalized filetype → (plugin id, RAW registration language);
    /// last registration wins (enable order).
    table: HashMap<String, (String, String)>,
    /// The effective table's change fingerprint (cache invalidation).
    fingerprint: u64,
    /// The booted host (the bounded `render_markdown_block` calls).
    host: Option<Arc<PluginHost>>,
    /// (filetype, content hash, fingerprint, syntax hash) → rows.
    cache: HashMap<(String, u64, u64, u64), CachedRows>,
    /// Keys with an in-flight render (no duplicate spawns).
    inflight: HashSet<(String, u64, u64, u64)>,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(State {
            table: HashMap::new(),
            fingerprint: 0,
            host: None,
            cache: HashMap::new(),
            inflight: HashSet::new(),
        })
    })
}

/// Bumped every time a render reply lands in the cache (the pump's
/// repaint trigger).
static RENDERED_VERSION: AtomicU64 = AtomicU64::new(0);

/// The last render-landing version (the pump compares; a change
/// requests a redraw).
#[must_use]
pub fn rendered_version() -> u64 {
    RENDERED_VERSION.load(Ordering::Acquire)
}

/// Install the dispatch table from the registration snapshot (raw
/// languages normalize here — the consumption side of the wire).
/// Cache entries from a DIFFERENT table fingerprint are dropped (the
/// ref rebuilds the merged renderer map on change only).
pub fn install_table(host: Option<Arc<PluginHost>>, renderers: &[(String, String)]) {
    let mut table: HashMap<String, (String, String)> = HashMap::new();
    for (plugin_id, language) in renderers {
        if let Some(filetype) = opentui_core::tree_sitter::info_string_to_filetype(language) {
            table.insert(filetype, (plugin_id.clone(), language.clone()));
        }
    }
    let fingerprint = {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        let mut keys: Vec<_> = table.iter().collect();
        keys.sort_by(|a, b| a.0.cmp(b.0));
        for (filetype, (plugin, language)) in keys {
            filetype.hash(&mut hash);
            plugin.hash(&mut hash);
            language.hash(&mut hash);
        }
        hash.finish()
    };
    if let Ok(mut state) = state().lock() {
        if state.fingerprint != fingerprint {
            state.cache.clear();
            // The in-flight dedup keys embed the OLD fingerprint — a
            // pending render's reply would land under a stale key, so
            // drop the dedup map too (an in-flight thread's insert
            // under the new fingerprint is harmless).
            state.inflight.clear();
            state.fingerprint = fingerprint;
        }
        state.table = table;
        state.host = host;
    }
}

/// The pipeline's consult: the owning plugin, the RAW registration
/// language (the render key), and the table fingerprint.
#[must_use]
pub fn lookup(filetype: &str) -> Option<(String, String, u64)> {
    let state = state().lock().ok()?;
    let (plugin, language) = state.table.get(filetype)?;
    Some((plugin.clone(), language.clone(), state.fingerprint))
}

/// The cache consult for one (filetype, content) pair. `None` = miss
/// (fire [`spawn_render`]); `Some(Err(()))` = permanent failure (the
/// default path stays).
#[must_use]
pub fn cached_rows(
    filetype: &str,
    fingerprint: u64,
    content_hash: u64,
    syntax_hash: u64,
) -> Option<CachedRows> {
    let state = state().lock().ok()?;
    state.cache.get(&(filetype.to_owned(), content_hash, fingerprint, syntax_hash)).cloned()
}

/// Fire the bounded off-loop render for a cache miss. Returns
/// immediately; the reply lands in the cache and bumps
/// [`rendered_version`]. Failures cache permanently (logged once).
#[allow(clippy::too_many_arguments)]
pub fn spawn_render(
    plugin_id: String,
    language: String,
    code: String,
    filetype: String,
    fingerprint: u64,
    content_hash: u64,
    syntax_hash: u64,
    width: usize,
) {
    let key = (filetype, content_hash, fingerprint, syntax_hash);
    let host = {
        let Ok(mut state) = state().lock() else { return };
        if state.inflight.contains(&key) {
            return;
        }
        let Some(host) = state.host.clone() else { return };
        state.inflight.insert(key.clone());
        host
    };
    let failure_key = key.clone();
    let builder = std::thread::Builder::new().name("oc-tui-plugin-markdown".into());
    let spawned = builder.spawn(move || {
        let result =
            host.render_markdown_block(&plugin_id, &language, &code, width, RENDER_TIMEOUT);
        let Ok(mut state) = state().lock() else { return };
        state.inflight.remove(&key);
        if state.cache.len() >= CACHE_CAPACITY {
            state.cache.clear();
        }
        match result {
            Ok(spec) => {
                state.cache.insert(key, Ok(spec.rows));
            }
            Err(error) => {
                // Permanent for this (code, fingerprint) pair — log
                // once, never retry.
                crate::keymap::log_skip(&format!(
                    "plugin markdown render {plugin_id}/{language}: {error}"
                ));
                state.cache.insert(key, Err(()));
            }
        }
        RENDERED_VERSION.fetch_add(1, Ordering::Release);
    });
    if spawned.is_err() {
        if let Ok(mut state) = state().lock() {
            state.inflight.remove(&failure_key);
        }
    }
}

/// Test seam: reset the whole registry (table, host, cache).
pub fn reset_for_test() {
    if let Ok(mut state) = state().lock() {
        state.table.clear();
        state.fingerprint = 0;
        state.host = None;
        state.cache.clear();
        state.inflight.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cache invalidation on unregister (a table change): a seeded
    /// HIT reverts to a MISS when the renderer leaves the table —
    /// the fingerprint changes and the cache (keyed by it) drops,
    /// so the pipeline falls back to the default path.
    #[test]
    fn cache_invalidates_when_the_table_changes() {
        reset_for_test();
        install_table(None, &[("plugin-a".to_owned(), "wormlang".to_owned())]);
        let Some((_, _, fingerprint)) = lookup("wormlang") else {
            panic!("the renderer registered");
        };
        // Seed the cache the way a landed render reply would.
        {
            let Ok(mut state) = state().lock() else { panic!("state lock") };
            state
                .cache
                .insert(("wormlang".to_owned(), 7, fingerprint, 11), Ok(vec!["plugin row".into()]));
        }
        assert!(cached_rows("wormlang", fingerprint, 7, 11).is_some(), "the seeded entry hits");

        // The renderer unregistered (a snapshot without it): the same
        // (content, syntax) pair must MISS — the default path renders.
        install_table(None, &[]);
        assert!(lookup("wormlang").is_none(), "the renderer left the table");
        assert!(
            cached_rows("wormlang", fingerprint, 7, 11).is_none(),
            "the cache dropped on the table change"
        );
        reset_for_test();
    }

    /// The in-flight dedup map clears together with the cache on a
    /// table change (its keys embed the OLD fingerprint).
    #[test]
    fn inflight_clears_on_table_change() {
        reset_for_test();
        install_table(None, &[("plugin-a".to_owned(), "wormlang".to_owned())]);
        {
            let Ok(mut state) = state().lock() else { panic!("state lock") };
            let fingerprint = state.fingerprint;
            state.inflight.insert(("wormlang".to_owned(), 1, fingerprint, 2));
            assert_eq!(state.inflight.len(), 1, "the dedup entry landed");
        }
        install_table(None, &[]);
        {
            let Ok(state) = state().lock() else { panic!("state lock") };
            assert!(state.inflight.is_empty(), "inflight cleared with the cache");
        }
        reset_for_test();
    }
}
