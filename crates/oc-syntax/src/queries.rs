//! The grammar registry: the static filetype → (grammar, queries) table,
//! vendored `.scm` query sources, and `OnceLock`-cached `Query`/`Parser`
//! construction. Mirrors `packages/tui/src/parsers-config.ts` (the catalog
//! shape and the v2 alias convention — `diff`/`udiff`/`patch`), minus the
//! runtime wasm fetching: everything is compiled in and resolved offline.
//!
//! ## The full catalog (R-01b + G-08)
//!
//! 20 filetypes over the v2 catalog's core set (the oracle-curated list)
//! plus v2's four tail languages the G-08 lane registered (lua, vim,
//! scala, make). Routing follows v2's notes: python/php/html highlights
//! from the parser repo (the nvim queries are marked broken at
//! parsers-config.ts:12-16, 129-131, 149-151) — everything else is
//! vendored from the parser repo's own query **at the exact crate tag**
//! (guaranteed grammar/query ABI coherence; nvim master is a moving
//! target). toml and vim ship as vendored path crates under
//! `third_party/` (their lineages carry no tree-sitter-language
//! release on crates.io at the pinned tags); their queries sit at the
//! same tags as the C sources. swift/c-sharp/kotlin were implemented
//! and then DROPPED for release-binary size (ROADMAP R-01b: 72.7 MiB
//! vs the 67.6 ceiling; the three heaviest grammars); RA-07 re-added
//! them behind the non-default `lang-swift`/`lang-csharp`/
//! `lang-kotlin` cargo features, so the default release binary is
//! unchanged.
//!
//! ## RA-07 — the 17-language gap close
//!
//! 14 default-on additions (bash, julia, elixir, xml, agda, r, hcl,
//! haskell, ocaml, fsharp, clojure, nix, markdown, vue) with queries
//! vendored at each grammar's exact pin (the repo's own query wherever
//! one exists; hcl v1.1.0 ships none, so the nvim-treesitter query —
//! the ref's own route — is vendored). vue is a vendored path crate
//! (the anomalyco fork never hit crates.io). markdown rides the
//! tree-sitter-md split crate: block grammar on the `markdown`
//! filetype, inline grammar on `markdown-inline` — DEGRADED vs the
//! ref, which uses opentui's built-in with code-fence language
//! injections; this port has no injection support, so fenced code
//! inside markdown blocks renders with the markdown query only (inner
//! languages stay plain). Three more (swift, csharp, kotlin) ride
//! behind non-default cargo features.

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
    /// `tree-sitter-go` v0.25.0 `queries/highlights.scm`.
    pub const GO_HIGHLIGHTS: &str = include_str!("../queries/go-highlights.scm");
    /// `tree-sitter-c` v0.24.2 `queries/highlights.scm`.
    pub const C_HIGHLIGHTS: &str = include_str!("../queries/c-highlights.scm");
    /// `tree-sitter-cpp` v0.23.4 `queries/highlights.scm` (self-contained
    /// — the parser repo inlines the C base patterns; only nvim's
    /// variant `; inherits: c`).
    pub const CPP_HIGHLIGHTS: &str = include_str!("../queries/cpp-highlights.scm");
    /// `tree-sitter-java` v0.23.5 `queries/highlights.scm`.
    pub const JAVA_HIGHLIGHTS: &str = include_str!("../queries/java-highlights.scm");
    /// `tree-sitter-json` v0.24.8 `queries/highlights.scm`.
    pub const JSON_HIGHLIGHTS: &str = include_str!("../queries/json-highlights.scm");
    /// `tree-sitter-yaml` (tree-sitter-grammars) v0.7.2
    /// `queries/highlights.scm`.
    pub const YAML_HIGHLIGHTS: &str = include_str!("../queries/yaml-highlights.scm");
    /// `tree-sitter-toml` (tree-sitter-grammars) v0.7.0
    /// `queries/highlights.scm` (grammar vendored at the same tag under
    /// `third_party/tree-sitter-toml`).
    pub const TOML_HIGHLIGHTS: &str = include_str!("../queries/toml-highlights.scm");
    /// `tree-sitter-html` v0.23.2 `queries/highlights.scm` (v2 routes
    /// html highlights from the parser repo — parsers-config.ts:149-151).
    pub const HTML_HIGHLIGHTS: &str = include_str!("../queries/html-highlights.scm");
    /// `tree-sitter-css` v0.25.0 `queries/highlights.scm`.
    pub const CSS_HIGHLIGHTS: &str = include_str!("../queries/css-highlights.scm");
    /// `tree-sitter-diff` (tree-sitter-grammars) v0.1.0
    /// `queries/highlights.scm` — the tag IS the exact commit v2 pins
    /// (2520c3f…, parsers-config.ts:300-308).
    pub const DIFF_HIGHLIGHTS: &str = include_str!("../queries/diff-highlights.scm");
    /// `tree-sitter-javascript` v0.23.1 `queries/highlights.scm` — the
    /// same file composed into the typescript query above, standing on
    /// its own for the `javascript` filetype (crate pinned at the
    /// matching v0.23.1).
    pub const JAVASCRIPT_HIGHLIGHTS: &str = include_str!("../queries/javascript-highlights.scm");
    /// `tree-sitter-ruby` v0.23.1 `queries/highlights.scm`.
    pub const RUBY_HIGHLIGHTS: &str = include_str!("../queries/ruby-highlights.scm");
    /// `tree-sitter-ruby` v0.23.1 `queries/locals.scm`.
    pub const RUBY_LOCALS: &str = include_str!("../queries/ruby-locals.scm");
    /// `tree-sitter-php` v0.25.1 `queries/highlights.scm` (v2 routes php
    /// highlights from the parser repo — parsers-config.ts:129-131).
    pub const PHP_HIGHLIGHTS: &str = include_str!("../queries/php-highlights.scm");
    /// `tree-sitter-lua` (tree-sitter-grammars) v0.5.0
    /// `queries/highlights.scm` — the exact wasm tag v2 pins
    /// (parsers-config.ts:234-241).
    pub const LUA_HIGHLIGHTS: &str = include_str!("../queries/lua-highlights.scm");
    /// `tree-sitter-lua` v0.5.0 `queries/locals.scm` (the same tag's).
    pub const LUA_LOCALS: &str = include_str!("../queries/lua-locals.scm");
    /// `tree-sitter-vim` (tree-sitter-grammars) v0.8.1
    /// `queries/vim/highlights.scm` — the grammar repo's own query at
    /// the exact v2 pin (parsers-config.ts:352-363 routes nvim master;
    /// this port vendors at-tag for ABI coherence, like every other
    /// entry).
    pub const VIM_HIGHLIGHTS: &str = include_str!("../queries/vim-highlights.scm");
    /// `tree-sitter-scala` v0.24.0 `queries/highlights.scm` — the
    /// grammar repo's own query at the exact v2 pin
    /// (parsers-config.ts:136-143; scala's rust bindings ship from the
    /// same repo).
    pub const SCALA_HIGHLIGHTS: &str = include_str!("../queries/scala-highlights.scm");
    /// `tree-sitter-make` (tree-sitter-grammars) v1.1.1
    /// `queries/highlights.scm` — the grammar repo's own query at the
    /// exact v2 pin (parsers-config.ts:342-351).
    pub const MAKE_HIGHLIGHTS: &str = include_str!("../queries/make-highlights.scm");
    /// `tree-sitter-bash` v0.25.0 `queries/highlights.scm` (RA-07; the
    /// exact v2 wasm pin, parsers-config.ts:72-79).
    pub const BASH_HIGHLIGHTS: &str = include_str!("../queries/bash-highlights.scm");
    /// `tree-sitter-julia` v0.23.1 `queries/highlights.scm` (RA-07;
    /// exact v2 pin, parsers-config.ts:225-232).
    pub const JULIA_HIGHLIGHTS: &str = include_str!("../queries/julia-highlights.scm");
    /// `tree-sitter-elixir` v0.3.5 `queries/highlights.scm` (RA-07;
    /// exact v2 pin, parsers-config.ts:310-320).
    pub const ELIXIR_HIGHLIGHTS: &str = include_str!("../queries/elixir-highlights.scm");
    /// `tree-sitter-xml` (tree-sitter-grammars) v0.7.0
    /// `queries/xml/highlights.scm` (RA-07; exact v2 pin,
    /// parsers-config.ts:365-375).
    pub const XML_HIGHLIGHTS: &str = include_str!("../queries/xml-highlights.scm");
    /// `tree-sitter-agda` v1.3.3 `queries/highlights.scm` (RA-07;
    /// exact v2 pin, parsers-config.ts:377-384).
    pub const AGDA_HIGHLIGHTS: &str = include_str!("../queries/agda-highlights.scm");
    /// `tree-sitter-r` v1.2.0 `queries/highlights.scm` (RA-07; the
    /// exact v2 wasm pin, parsers-config.ts:331-341).
    pub const R_HIGHLIGHTS: &str = include_str!("../queries/r-highlights.scm");
    /// `tree-sitter-r` v1.2.0 `queries/locals.scm` (same tag).
    pub const R_LOCALS: &str = include_str!("../queries/r-locals.scm");
    /// nvim-treesitter master `queries/hcl/highlights.scm` (RA-07) —
    /// hcl v1.1.0 (the crate closest to the ref's v1.2.0 wasm) ships
    /// no queries in its repo, and the ref itself routes nvim here
    /// (parsers-config.ts:180-187).
    pub const HCL_HIGHLIGHTS: &str = include_str!("../queries/hcl-highlights.scm");
    /// `tree-sitter-haskell` v0.23.1 `queries/highlights.scm` (RA-07;
    /// exact v2 pin, parsers-config.ts:207-214).
    pub const HASKELL_HIGHLIGHTS: &str = include_str!("../queries/haskell-highlights.scm");
    /// `tree-sitter-ocaml` v0.24.2 `queries/ocaml/highlights.scm`
    /// (RA-07; exact v2 pin, parsers-config.ts:244-252).
    pub const OCAML_HIGHLIGHTS: &str = include_str!("../queries/ocaml-highlights.scm");
    /// `tree-sitter-fsharp` v0.3.0 `queries/highlights.scm` (RA-07;
    /// exact v2 pin, parsers-config.ts:322-330).
    pub const FSHARP_HIGHLIGHTS: &str = include_str!("../queries/fsharp-highlights.scm");
    /// `tree-sitter-clojure` v0.1.0 `grammar-src/queries/highlights.scm`
    /// (RA-07) — the query shipped inside the published crate itself
    /// (the crates.io grammar-src submodule carries it; the ref routes
    /// the equivalent nvim query, parsers-config.ts:253-261).
    pub const CLOJURE_HIGHLIGHTS: &str = include_str!("../queries/clojure-highlights.scm");
    /// `tree-sitter-nix` (nix-community) v0.3.0
    /// `queries/highlights.scm` (RA-07; same lineage as the v2 wasm,
    /// parsers-config.ts:286-299).
    pub const NIX_HIGHLIGHTS: &str = include_str!("../queries/nix-highlights.scm");
    /// `tree-sitter-md` v0.5.4 (MDeiml/tree-sitter-markdown) block
    /// grammar `queries/highlights.scm` (RA-07). DEGRADED vs the ref:
    /// the ref's opentui built-in injects inner languages into code
    /// fences; this port has no injection support, so fenced code
    /// inside markdown renders with the markdown query only (inner
    /// languages stay plain) — injections.scm is deliberately NOT
    /// vendored.
    pub const MARKDOWN_HIGHLIGHTS: &str = include_str!("../queries/markdown-highlights.scm");
    /// `tree-sitter-md` v0.5.4 inline grammar
    /// `queries/highlights.scm` (the split parser's inline half,
    /// registered on its own `markdown-inline` filetype).
    pub const MARKDOWN_INLINE_HIGHLIGHTS: &str =
        include_str!("../queries/markdown-inline-highlights.scm");
    /// anomalyco/tree-sitter-vue v0.1.2 `queries/html_tags/highlights.scm`
    /// (RA-07; the first half of the ref's composed vue query,
    /// parsers-config.ts:170-178 — the fork never published to
    /// crates.io, so the grammar is vendored under third_party/).
    pub const VUE_HTML_TAGS_HIGHLIGHTS: &str =
        include_str!("../queries/vue-html-tags-highlights.scm");
    /// anomalyco/tree-sitter-vue v0.1.2 `queries/vue/highlights.scm` —
    /// the second half of the composed query (its `; inherits:
    /// html_tags` dependency is satisfied by the file above, prefixed
    /// per the ref's concat order).
    pub const VUE_HIGHLIGHTS: &str = include_str!("../queries/vue-highlights.scm");
    /// `tree-sitter-swift` (alex-pinkus lineage) v0.7.1
    /// `queries/highlights.scm` — the exact v2 pin
    /// (parsers-config.ts:263-275); behind the `lang-swift` feature.
    #[cfg(feature = "lang-swift")]
    pub const SWIFT_HIGHLIGHTS: &str = include_str!("../queries/swift-highlights.scm");
    /// `tree-sitter-c-sharp` v0.23.1 `queries/highlights.scm` — the
    /// exact v2 pin (parsers-config.ts:60-70); behind the
    /// `lang-csharp` feature.
    #[cfg(feature = "lang-csharp")]
    pub const CSHARP_HIGHLIGHTS: &str = include_str!("../queries/csharp-highlights.scm");
    /// fwcd/tree-sitter-kotlin 0.3.8 `queries/highlights.scm` — the
    /// exact v2 pin (parsers-config.ts:105-110); behind the
    /// `lang-kotlin` feature.
    #[cfg(feature = "lang-kotlin")]
    pub const KOTLIN_HIGHLIGHTS: &str = include_str!("../queries/kotlin-highlights.scm");
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

/// The static registry — the R-01 spike's 3 languages, the R-01b
/// catalog extension (13 more grammars — javascript rides the
/// already-vendored typescript query), the G-08 tail (lua, vim,
/// scala, make), and the RA-07 gap close (14 default-on adds +
/// swift/csharp/kotlin behind their `lang-*` features — all 37 ref
/// languages register with every feature enabled). Built once into a
/// leaked Vec so cargo-feature-gated entries can join the table.
#[rustfmt::skip] // one flat table: filetype, aliases, grammar, queries — scannable
#[allow(clippy::too_many_lines)] // a 35-entry data table, not logic
pub fn registry() -> &'static [GrammarEntry] {
    static REGISTRY: OnceLock<Vec<GrammarEntry>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            let mut entries = vec![
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
        GrammarEntry {
            filetype: "go",
            aliases: &["golang"],
            language: tree_sitter_go::LANGUAGE,
            highlight_query: scm::GO_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "c",
            aliases: &["h"],
            language: tree_sitter_c::LANGUAGE,
            highlight_query: scm::C_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "cpp",
            aliases: &["c++", "cxx", "cc", "hpp", "hxx"],
            language: tree_sitter_cpp::LANGUAGE,
            highlight_query: scm::CPP_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "java",
            aliases: &[],
            language: tree_sitter_java::LANGUAGE,
            highlight_query: scm::JAVA_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "json",
            aliases: &[],
            language: tree_sitter_json::LANGUAGE,
            highlight_query: scm::JSON_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "yaml",
            aliases: &["yml"],
            language: tree_sitter_yaml::LANGUAGE,
            highlight_query: scm::YAML_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "toml",
            aliases: &[],
            language: tree_sitter_toml::LANGUAGE,
            highlight_query: scm::TOML_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "html",
            aliases: &["htm"],
            language: tree_sitter_html::LANGUAGE,
            highlight_query: scm::HTML_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "css",
            aliases: &[],
            language: tree_sitter_css::LANGUAGE,
            highlight_query: scm::CSS_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "diff",
            aliases: &["udiff", "patch"], // the v2 alias convention
            language: tree_sitter_diff::LANGUAGE,
            highlight_query: scm::DIFF_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "javascript",
            aliases: &["js", "mjs", "cjs"],
            language: tree_sitter_javascript::LANGUAGE,
            highlight_query: scm::JAVASCRIPT_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "ruby",
            aliases: &["rb"],
            language: tree_sitter_ruby::LANGUAGE,
            highlight_query: scm::RUBY_HIGHLIGHTS,
            locals_query: Some(scm::RUBY_LOCALS),
        },
        GrammarEntry {
            filetype: "php",
            aliases: &[],
            language: tree_sitter_php::LANGUAGE_PHP,
            highlight_query: scm::PHP_HIGHLIGHTS,
            locals_query: None,
        },
        // -- G-08: v2's tail languages (lua, vim, scala, make) --------
        GrammarEntry {
            filetype: "lua",
            aliases: &[],
            language: tree_sitter_lua::LANGUAGE,
            highlight_query: scm::LUA_HIGHLIGHTS,
            locals_query: Some(scm::LUA_LOCALS),
        },
        GrammarEntry {
            filetype: "vim",
            aliases: &["vi"],
            language: tree_sitter_vim::LANGUAGE,
            highlight_query: scm::VIM_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "scala",
            aliases: &[],
            language: tree_sitter_scala::LANGUAGE,
            highlight_query: scm::SCALA_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "make",
            aliases: &["makefile", "gnumake"],
            language: tree_sitter_make::LANGUAGE,
            highlight_query: scm::MAKE_HIGHLIGHTS,
            locals_query: None,
        },
        // -- RA-07: the language-gap close ----------------------------
        GrammarEntry {
            filetype: "bash",
            aliases: &["sh", "shell", "zsh"],
            language: tree_sitter_bash::LANGUAGE,
            highlight_query: scm::BASH_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "julia",
            aliases: &["jl"],
            language: tree_sitter_julia::LANGUAGE,
            highlight_query: scm::JULIA_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "elixir",
            aliases: &["ex", "exs"], // ex/exs ride the elixir grammar (port extras)
            language: tree_sitter_elixir::LANGUAGE,
            highlight_query: scm::ELIXIR_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "xml",
            aliases: &["svg", "xsd"], // svg/xsd are xml documents (port extras)
            language: tree_sitter_xml::LANGUAGE_XML,
            highlight_query: scm::XML_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "agda",
            aliases: &[],
            language: tree_sitter_agda::LANGUAGE,
            highlight_query: scm::AGDA_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "r",
            aliases: &[],
            language: tree_sitter_r::LANGUAGE,
            highlight_query: scm::R_HIGHLIGHTS,
            locals_query: Some(scm::R_LOCALS),
        },
        GrammarEntry {
            filetype: "hcl",
            aliases: &["terraform", "tf"], // tf/terraform are hcl dialects (port extras)
            language: tree_sitter_hcl::LANGUAGE,
            highlight_query: scm::HCL_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "haskell",
            aliases: &["hs"],
            language: tree_sitter_haskell::LANGUAGE,
            highlight_query: scm::HASKELL_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "ocaml",
            aliases: &["ml"],
            language: tree_sitter_ocaml::LANGUAGE_OCAML,
            highlight_query: scm::OCAML_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "fsharp",
            aliases: &["fs", "f#"], // fs/f# fence spellings (port extras)
            language: tree_sitter_fsharp::LANGUAGE_FSHARP,
            highlight_query: scm::FSHARP_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "clojure",
            aliases: &["clj", "cljs", "edn"], // clj/cljs/edn fence spellings (port extras)
            language: tree_sitter_clojure::LANGUAGE,
            highlight_query: scm::CLOJURE_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "nix",
            aliases: &[],
            language: tree_sitter_nix::LANGUAGE,
            highlight_query: scm::NIX_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "markdown",
            aliases: &["md"],
            // DEGRADED vs the ref: no injection support — code-fence
            // inner languages inside markdown render plain (see the
            // scm::MARKDOWN_HIGHLIGHTS doc above).
            language: tree_sitter_md::LANGUAGE,
            highlight_query: scm::MARKDOWN_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            // The split parser's inline half on its own fence label —
            // mostly registered so the query-construction test pins its ABI.
            filetype: "markdown-inline",
            aliases: &["md-inline"],
            language: tree_sitter_md::INLINE_LANGUAGE,
            highlight_query: scm::MARKDOWN_INLINE_HIGHLIGHTS,
            locals_query: None,
        },
        GrammarEntry {
            filetype: "vue",
            aliases: &[],
            language: tree_sitter_vue::LANGUAGE,
            // the ref's composed query: html_tags first, vue overlays
            // (later patterns overlay — the concat order mirrors the
            // ref's array order, parsers-config.ts:173-177). Composed
            // at compile time over include_str! literals, like the
            // typescript query above.
            highlight_query: concat!(
                "\n",
                include_str!("../queries/vue-html-tags-highlights.scm"),
                "\n",
                include_str!("../queries/vue-highlights.scm"),
            ),
            locals_query: None,
        },
            ];
            // -- RA-07 feature-gated re-adds (R-01b size drops) -------
            #[cfg(feature = "lang-swift")]
            entries.push(GrammarEntry {
                filetype: "swift",
                aliases: &[],
                language: tree_sitter_swift::LANGUAGE,
                highlight_query: scm::SWIFT_HIGHLIGHTS,
                locals_query: None,
            });
            #[cfg(feature = "lang-csharp")]
            entries.push(GrammarEntry {
                filetype: "csharp",
                aliases: &["c#", "cs"],
                language: tree_sitter_c_sharp::LANGUAGE,
                highlight_query: scm::CSHARP_HIGHLIGHTS,
                locals_query: None,
            });
            #[cfg(feature = "lang-kotlin")]
            entries.push(GrammarEntry {
                filetype: "kotlin",
                aliases: &["kt"],
                language: tree_sitter_kotlin::LANGUAGE,
                highlight_query: scm::KOTLIN_HIGHLIGHTS,
                locals_query: None,
            });
            entries
        })
        .as_slice()
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
    /// R-01b: auto-covers every catalog addition.
    #[test]
    fn every_registry_query_constructs() {
        // 20 core filetypes + the 15 default-on RA-07 registrations
        // (14 ref languages + the markdown split's inline half on its
        // own label); the three R-01b size drops (swift/c-sharp/
        // kotlin) re-add only behind their cargo features — all 37 ref
        // languages + markdown-inline with every feature enabled (see
        // ROADMAP R-01b + the RA-07 report).
        #[cfg(not(any(feature = "lang-swift", feature = "lang-csharp", feature = "lang-kotlin")))]
        assert_eq!(registry().len(), 35, "20 core + 15 RA-07 registrations (swift/c-sharp/kotlin behind lang-* features; see ROADMAP R-01b)");
        #[cfg(all(feature = "lang-swift", feature = "lang-csharp", feature = "lang-kotlin"))]
        assert_eq!(registry().len(), 38, "all 37 ref languages + markdown-inline with every lang-* feature enabled");
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
        // Unknown → None (offline rule: instant, no parse). perl is
        // not in the v2 catalog (ocaml WAS added by RA-07).
        assert!(lookup("perl").is_none());
        assert!(lookup("").is_none());
        assert!(lookup("   ").is_none());
        // The v2 catalog convention: every canonical filetype and alias
        // resolves (to its OWN entry — no cross-entry shadowing), and the
        // v2 alias pairs hold: diff/udiff/patch, make/makefile (G-08
        // registered the tail languages at their ref pins).
        for entry in registry() {
            assert_eq!(
                lookup(entry.filetype).map(|found| found.filetype),
                Some(entry.filetype),
                "{} resolves",
                entry.filetype
            );
            for alias in entry.aliases {
                assert_eq!(
                    lookup(alias).map(|found| found.filetype),
                    Some(entry.filetype),
                    "alias {alias} routes to {}",
                    entry.filetype
                );
            }
        }
        assert_eq!(lookup("udiff").map(|entry| entry.filetype), Some("diff"));
        assert_eq!(lookup("patch").map(|entry| entry.filetype), Some("diff"));
        assert_eq!(lookup("yml").map(|entry| entry.filetype), Some("yaml"));
        assert_eq!(lookup("js").map(|entry| entry.filetype), Some("javascript"));
        assert_eq!(lookup("rb").map(|entry| entry.filetype), Some("ruby"));
        // G-08 tail: the ref catalog entries resolve (parsers-config.ts
        // make aliases makefile; vi/gnumake are the port's extras).
        assert_eq!(lookup("makefile").map(|entry| entry.filetype), Some("make"));
        assert_eq!(lookup("gnumake").map(|entry| entry.filetype), Some("make"));
        assert_eq!(lookup("vi").map(|entry| entry.filetype), Some("vim"));
        assert!(lookup("sbt").is_none(), "sbt is NOT scala (the G-08 ruling)");
    }
}
