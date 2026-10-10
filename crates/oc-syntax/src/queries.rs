//! The grammar registry: the static filetype → (grammar, queries) table,
//! vendored `.scm` query sources, and `OnceLock`-cached `Query`/`Parser`
//! construction. Mirrors `packages/tui/src/parsers-config.ts` (the catalog
//! shape and the v2 alias convention — `diff`/`udiff`/`patch`,
//! `make`/`makefile`), minus the runtime wasm fetching: everything is
//! compiled in and resolved offline.

use std::sync::{Arc, OnceLock};

use tree_sitter::{Parser, Query};
use tree_sitter_language::LanguageFn;

/// Vendored highlight query sources (`include_str!`, no runtime fetching).
mod scm {
    /// `tree-sitter-rust` v0.24.2 `queries/highlights.scm`.
    pub const RUST_HIGHLIGHTS: &str = include_str!("../queries/rust-highlights.scm");
    /// `tree-sitter-python` v0.25.0 `queries/highlights.scm` (v2 routes
    /// python highlights from the parser repo — parsers-config.ts:12-16).
    pub const PYTHON_HIGHLIGHTS: &str = include_str!("../queries/python-highlights.scm");
    /// The composed typescript highlight query: the javascript base
    /// (`tree-sitter-javascript` v0.23.1, the `; inherits: javascript`
    /// dependency of the TS query) then the TS-specific patterns
    /// (`tree-sitter-typescript` v0.23.2; later patterns overlay),
    /// concatenated entirely at compile time via `concat!` over
    /// `include_str!` literals — zero runtime cost.
    pub const TYPESCRIPT_COMPOSED: &str = concat!(
        "\n",
        include_str!("../queries/javascript-highlights.scm"),
        "\n",
        include_str!("../queries/typescript-highlights.scm"),
    );

    /// `tree-sitter-typescript` v0.23.2 `queries/locals.scm`.
    pub const TYPESCRIPT_LOCALS: &str = include_str!("../queries/typescript-locals.scm");
}

/// One registry entry: a filetype, its v2-style aliases, the native grammar
/// constructor, and the vendored query sources for it.
pub struct GrammarEntry {
    /// The canonical filetype (fence info string, normalized).
    pub filetype: &'static str,
    /// Alternate fence labels that route here (`aliases` in parsers-config).
    pub aliases: &'static [&'static str],
    /// The native grammar (the `wasm` URL + loader in v2).
    pub language: LanguageFn,
    /// The highlight query source (one or more `.scm` files composed in
    /// order — later patterns overlay earlier ones, like nvim's
    /// `; inherits:` concatenation).
    pub highlight_query: &'static str,
    /// The optional locals query source (registered but not executed for
    /// styling; built by the query-construction test to catch drift).
    pub locals_query: Option<&'static str>,
}

/// The static registry — the spike's 3 languages. The full v2 set (18
/// grammars + aliases) extends this table; the abort threshold for that is
/// +20 MiB on the release binary (see the spike report).
pub fn registry() -> &'static [GrammarEntry] {
    &[
        GrammarEntry {
            filetype: "rust",
            aliases: &["rs"],
            language: tree_sitter_rust::LANGUAGE,
            highlight_query: scm::RUST_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "python",
            aliases: &["py", "python3"],
            language: tree_sitter_python::LANGUAGE,
            highlight_query: scm::PYTHON_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "typescript",
            aliases: &["ts"],
            language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            highlight_query: scm::TYPESCRIPT_COMPOSED,
            locals_query: Some(scm::TYPESCRIPT_LOCALS),
        },
    ]
}

/// Normalize a fence info string to a canonical filetype token: the first
/// whitespace/comma-separated field (`"rust,ignore"` → `"rust"`), compared
/// ASCII-case-insensitively (no allocation — fence labels are tiny).
fn lookup_matches(filetype: &str, entry: &GrammarEntry) -> bool {
    let Some(token) = filetype.split([',', ' ', '\t']).find(|token| !token.is_empty()) else {
        return false;
    };
    token.eq_ignore_ascii_case(entry.filetype)
        || entry.aliases.iter().any(|alias| token.eq_ignore_ascii_case(alias))
}

/// Resolve a filetype (or any of its aliases) to its registry entry.
/// Unknown filetypes return `None` instantly — no parsing, no blocking,
/// no panics; the caller falls back to plain rendering.
#[must_use]
pub fn lookup(filetype: &str) -> Option<&'static GrammarEntry> {
    registry().iter().find(|entry| lookup_matches(filetype, entry))
}

/// One compiled-query cache slot: (filetype, kind) → query, where kind
/// distinguishes the highlight and locals queries of one grammar.
type QueryCacheSlot = (&'static str, &'static str, Arc<Query>);

/// The process-wide compiled-query cache (queries are immutable; compile
/// once per grammar).
fn query_cache() -> &'static std::sync::Mutex<Vec<QueryCacheSlot>> {
    static CACHE: OnceLock<std::sync::Mutex<Vec<QueryCacheSlot>>> = OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn cached_query(
    filetype: &'static str,
    kind: &'static str,
    build: impl FnOnce() -> Query,
) -> Arc<Query> {
    let mut cache = query_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, _, query)) =
        cache.iter().find(|(name, query_kind, _)| *name == filetype && *query_kind == kind)
    {
        return Arc::clone(query);
    }
    let query = Arc::new(build());
    cache.push((filetype, kind, Arc::clone(&query)));
    query
}

/// A grammar's compiled highlight query (cached on first use).
pub fn highlight_query(entry: &GrammarEntry) -> Arc<Query> {
    // A panic here means registry drift (grammar/query ABI mismatch) —
    // the query-construction test pins this; a panic at first use of a
    // language is a hard bug, not a degraded mode.
    cached_query(entry.filetype, "highlight", || {
        let language = tree_sitter::Language::new(entry.language);
        Query::new(&language, entry.highlight_query).expect("vendored highlight query compiles")
    })
}

/// A grammar's compiled locals query, when registered (cached on first use).
pub fn locals_query(entry: &GrammarEntry) -> Option<Arc<Query>> {
    let source = entry.locals_query?;
    Some(cached_query(entry.filetype, "locals", || {
        let language = tree_sitter::Language::new(entry.language);
        Query::new(&language, source).expect("vendored locals query compiles")
    }))
}

/// A parser for the entry's grammar (cheap to build; tree-sitter parsing
/// is single-threaded per parser, so a fresh parser per highlight call
/// avoids contention between renders).
pub fn parser(entry: &GrammarEntry) -> Option<Parser> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter::Language::new(entry.language)).ok()?;
    Some(parser)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every highlight (and locals) query in the registry must compile
    /// against its grammar — the grammar/query ABI drift tripwire. Any
    /// grammar bump that renames node types or drops tokens fails here.
    #[test]
    fn every_registry_query_constructs() {
        for entry in registry() {
            let language = tree_sitter::Language::new(entry.language);
            let highlight = Query::new(&language, entry.highlight_query)
                .unwrap_or_else(|error| panic!("{} highlight query: {error}", entry.filetype));
            assert!(!highlight.capture_names().is_empty(), "{} has captures", entry.filetype);
            if let Some(source) = entry.locals_query {
                Query::new(&language, source)
                    .unwrap_or_else(|error| panic!("{} locals query: {error}", entry.filetype));
            }
        }
    }

    #[test]
    fn lookup_resolves_filetypes_and_aliases() {
        assert_eq!(lookup("rust").map(|entry| entry.filetype), Some("rust"));
        assert_eq!(lookup("rs").map(|entry| entry.filetype), Some("rust"));
        assert_eq!(lookup("RUST").map(|entry| entry.filetype), Some("rust"));
        assert_eq!(lookup("python").map(|entry| entry.filetype), Some("python"));
        assert_eq!(lookup("py").map(|entry| entry.filetype), Some("python"));
        assert_eq!(lookup("typescript").map(|entry| entry.filetype), Some("typescript"));
        assert_eq!(lookup("ts").map(|entry| entry.filetype), Some("typescript"));
        // Fence info extras: only the first field counts.
        assert_eq!(lookup("rust,ignore").map(|entry| entry.filetype), Some("rust"));
        assert_eq!(lookup("rs no-run").map(|entry| entry.filetype), Some("rust"));
        // Unknown → None (offline rule: instant, no parse).
        assert!(lookup("ocaml").is_none());
        assert!(lookup("").is_none());
        assert!(lookup("   ").is_none());
        // The v2 alias convention exists in the table shape (udiff/patch,
        // makefile live on their entries when those grammars join).
        assert!(registry().iter().all(|entry| !entry.aliases.is_empty()));
    }
}
