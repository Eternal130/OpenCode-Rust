//! The syntax highlighting seam — the oc-tui side of the D1d engine
//! convergence. oc-syntax is now a **grammar+query vendor** (its
//! registry: per-filetype `LanguageFn` + vendored `.scm` queries +
//! aliases); the runtime highlight path is opentui-core's
//! `TreeSitterClient` (`highlight_once`), with the oc-syntax catalog
//! registered into the global client at startup
//! ([`register_parsers`], idempotent — `add_default_parsers` retains
//! same-filetype entries).
//!
//! This module is the Rust counterpart of v2's `util/filetype.ts`
//! (`filetype(path)` → the `<code filetype={…}>` prop) plus the
//! span materialization the four call sites share:
//!
//! * [`highlight`] — parse one code payload once for a filetype
//!   (content-keyed cache; unknown filetypes return `None` instantly
//!   — no grammar, no blocking, no panics — callers render plain
//!   lines, exactly today's behavior).
//! * [`HighlightedBlock::line_spans`] — materialize one pre-parsed
//!   line as oc-tui spans against the theme's flattened syntax
//!   styles ([`SyntaxStyles`], built from oc-theme's
//!   `generate_syntax` → `convert_theme_to_styles` pair). Pure
//!   lookup — never parses, never touches tree-sitter.
//!
//! ## The UTF-16 boundary
//!
//! `highlight_once` returns `SimpleHighlight` offsets in **UTF-16
//! code units** (the JS-side index space opentui pins for oracle
//! parity); [`build`] converts them to byte offsets through
//! opentui's `Utf16Map` and then to per-line char offsets — the
//! space the span materialization cuts runs in. CJK (1 UTF-16 unit,
//! 3 bytes) and emoji (2 units, 4 bytes) exercise every mismatch
//! class; the conversion tests pin them.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use oc_theme::SyntaxStyleDef;
use opentui_core::tree_sitter::{
    add_default_parsers, destroy_tree_sitter_client, get_tree_sitter_client, FiletypeParserOptions,
    LanguageProvider, SimpleHighlight, Utf16Map,
};

use crate::layout::StyledSpan;
use crate::style::{Color, Modifier, Style};

// ------------------------------------------------------------------
// The parser registration (startup assembly)
// ------------------------------------------------------------------

/// Register the oc-syntax catalog (per the compiled feature set)
/// into opentui's global tree-sitter client — same grammars, same
/// vendored queries as before the convergence, now executed by
/// `TreeSitterClient::highlight_once`. Idempotent: the process-wide
/// [`OnceLock`] plus `add_default_parsers`' retain semantics make
/// repeat calls (startup hook + lazy first-highlight) no-ops.
pub fn register_parsers() {
    ensure_registered();
}

fn ensure_registered() {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    REGISTERED.get_or_init(|| {
        let parsers = oc_syntax::registry()
            .iter()
            .map(|entry| FiletypeParserOptions {
                filetype: entry.filetype.to_owned(),
                aliases: entry.aliases.iter().map(|alias| (*alias).to_owned()).collect(),
                language: LanguageProvider::Raw(entry.language),
                highlights_query: entry.highlight_query.to_owned(),
                // The oc-syntax vendor catalog ships no injections
                // queries (the G-13 degradation, unchanged by this
                // lane — markdown fence inner languages stay plain).
                injections_query: None,
                injection_mapping: None,
            })
            .collect();
        // The client may have been initialized already by an opentui
        // consumer that bypassed this seam (the CodeRenderable path
        // calls `get_tree_sitter_client` directly). `add_default_parsers`
        // only fills the override list read at INITIALIZE time, so a
        // pre-initialized client would keep the built-in defaults
        // (zig) and every other filetype would highlight to `None`.
        // Reset once at this seam, then register: the next highlight
        // re-initializes with the oc-syntax catalog.
        destroy_tree_sitter_client();
        add_default_parsers(parsers);
    });
}

// ------------------------------------------------------------------
// The theme-facing style map (scope → style resolution)
// ------------------------------------------------------------------

/// The flattened scope → style map (`generate_syntax` →
/// `convert_theme_to_styles` output), shared cheaply across renders.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SyntaxStyles(Arc<BTreeMap<String, SyntaxStyleDef>>);

impl SyntaxStyles {
    /// Flatten a theme's generated rules (oc-theme's
    /// `convert_theme_to_styles` — the same generated rule table the
    /// opentui `SyntaxStyle` face consumes; one theme, one map).
    #[must_use]
    pub fn from_rules(rules: &[oc_theme::SyntaxRule]) -> Self {
        Self(Arc::new(oc_theme::convert_theme_to_styles(rules)))
    }

    /// An empty map (every capture falls back to the base style).
    #[must_use]
    pub fn empty() -> Self {
        Self(Arc::new(BTreeMap::new()))
    }

    /// Iterate the flattened (scope → definition) map.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &SyntaxStyleDef)> {
        self.0.iter()
    }

    fn get(&self, scope: &str) -> Option<&SyntaxStyleDef> {
        self.0.get(scope)
    }
}

/// Capture names that predate the scope vocabulary the theme
/// generates; normalized before matching (upstream parser-repo
/// queries still emit `@escape` where nvim emits `@string.escape`).
const CAPTURE_ALIASES: &[(&str, &str)] = &[("escape", "string.escape")];

/// Resolve a capture name through `styles`: exact scope, then
/// progressively shorter dotted prefixes (`variable.parameter` →
/// `variable`), then `None`.
fn resolve_def<'a>(styles: &'a SyntaxStyles, capture: &str) -> Option<&'a SyntaxStyleDef> {
    for (from, to) in CAPTURE_ALIASES {
        if capture == *from {
            if let Some(def) = styles.get(to) {
                return Some(def);
            }
        }
    }
    if let Some(def) = styles.get(capture) {
        return Some(def);
    }
    let mut scope = capture;
    while let Some((head, _)) = scope.rsplit_once('.') {
        scope = head;
        if let Some(def) = styles.get(scope) {
            return Some(def);
        }
    }
    None
}

/// Apply a [`SyntaxStyleDef`] on top of the base style (fg/bg
/// overlay, attributes added — the same overlay order the spans
/// painted with before the convergence).
fn apply_def(base: Style, def: &SyntaxStyleDef) -> Style {
    let mut style = base;
    if let Some(fg) = def.fg {
        style = style.fg(Color::Rgb(fg.r, fg.g, fg.b));
    }
    if let Some(bg) = def.bg {
        style = style.bg(Color::Rgb(bg.r, bg.g, bg.b));
    }
    let mut modifier = Modifier::empty();
    if def.bold.unwrap_or(false) {
        modifier |= Modifier::BOLD;
    }
    if def.italic.unwrap_or(false) {
        modifier |= Modifier::ITALIC;
    }
    if def.underline.unwrap_or(false) {
        modifier |= Modifier::UNDERLINED;
    }
    if def.dim.unwrap_or(false) {
        modifier |= Modifier::DIM;
    }
    if !modifier.is_empty() {
        style = style.add_modifier(modifier);
    }
    style
}

/// Resolve a capture name to a paint style: the matched scope's
/// style overlaid on `base`, or `base` itself when nothing matches.
fn resolve(styles: &SyntaxStyles, capture: &str, base: Style) -> Style {
    resolve_def(styles, capture).map_or(base, |def| apply_def(base, def))
}

// ------------------------------------------------------------------
// The highlighted block (span materialization)
// ------------------------------------------------------------------

/// One capture range on one line, in **char** offsets within that
/// line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CaptureSpan {
    start: usize,
    end: usize,
    /// Index into the block's capture-name arena.
    name: u32,
}

/// A parsed-and-queried code block: the block's lines plus per-line
/// capture ranges. Cheap to clone (an [`Arc`] over the immutable
/// payload); tree-sitter types never escape.
#[derive(Debug, Clone)]
pub struct HighlightedBlock(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    lines: Vec<String>,
    /// Per line: capture ranges in emission order (later ranges
    /// overlay earlier ones where they overlap — the overlay
    /// convention the vendored queries are written against).
    captures: Vec<Vec<CaptureSpan>>,
    /// Interned capture names (`@`-less, dotted).
    names: Vec<Box<str>>,
}

impl HighlightedBlock {
    /// The block's line count (a `line` argument >= this renders
    /// empty).
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.0.lines.len()
    }

    /// Materialize one line as styled spans: `base` for uncaptured
    /// gaps, the theme-matched capture style overlaid everywhere else
    /// (the def's fg **and bg** overlay the base, matching the
    /// pre-convergence engine — the markdown fence path lets the diff
    /// scopes paint their row backgrounds; the path-shaped consumers
    /// pin the bg afterwards). Adjacent equal styles merge; the
    /// concatenated text always equals the source line — styling
    /// never changes glyphs.
    #[must_use]
    pub fn line_spans(&self, line: usize, base: Style, syntax: &SyntaxStyles) -> Vec<StyledSpan> {
        let Some(text) = self.0.lines.get(line) else {
            return Vec::new();
        };
        let ranges = &self.0.captures[line];
        if ranges.is_empty() {
            return vec![StyledSpan { text: text.clone(), style: base, link: None }];
        }
        let resolved: Vec<Style> =
            self.0.names.iter().map(|name| resolve(syntax, name, base)).collect();
        let chars: Vec<char> = text.chars().collect();
        let mut cells: Vec<Option<u32>> = vec![None; chars.len()];
        for range in ranges {
            for cell in &mut cells[range.start..range.end] {
                *cell = Some(range.name);
            }
        }
        // Cut the per-char table into contiguous same-style runs.
        let mut spans: Vec<StyledSpan> = Vec::new();
        let mut run = String::with_capacity(text.len());
        let mut current = base;
        for (ch, cell) in chars.iter().zip(&cells) {
            let style = cell.map_or(base, |name| resolved[name as usize]);
            if style != current {
                if !run.is_empty() {
                    spans.push(StyledSpan {
                        text: std::mem::take(&mut run),
                        style: current,
                        link: None,
                    });
                }
                current = style;
            }
            run.push(*ch);
        }
        if !run.is_empty() {
            spans.push(StyledSpan { text: run, style: current, link: None });
        }
        spans
    }
}

/// Convert one `highlight_once` result (UTF-16 offsets, document
/// space) into the per-line char-offset capture table.
fn build(content: &str, highlights: &[SimpleHighlight]) -> HighlightedBlock {
    let lines: Vec<String> = content.lines().map(str::to_owned).collect();
    let map = Utf16Map::new(content);

    // Line → start byte, built once: each line's start offset in the
    // source (single '\n' separators — `lines.join("\n")` space).
    let mut line_starts: Vec<usize> = Vec::with_capacity(lines.len() + 1);
    let mut offset = 0usize;
    for line in &lines {
        line_starts.push(offset);
        offset += line.len() + 1;
    }
    line_starts.push(offset);

    let mut names: Vec<Box<str>> = Vec::new();
    let mut name_index: HashMap<Box<str>, u32> = HashMap::new();
    let mut captures: Vec<Vec<CaptureSpan>> = vec![Vec::new(); lines.len()];

    for highlight in highlights {
        // UTF-16 → byte (the D1d boundary: SimpleHighlight offsets
        // are UTF-16 code units; the map rounds in-bounds).
        let start_byte = map.byte_of_utf16(highlight.start).min(content.len());
        let end_byte = map.byte_of_utf16(highlight.end).min(content.len()).max(start_byte);
        if start_byte == end_byte {
            continue; // zero-width (e.g. missing nodes in recovery)
        }
        let name: Box<str> = highlight.group.as_str().into();
        let index = *name_index.entry(name.clone()).or_insert_with(|| {
            names.push(name);
            u32::try_from(names.len() - 1).expect("capture name arena fits u32")
        });
        // Split the range across the lines it intersects; store char
        // offsets within each line.
        let first_line = line_starts.partition_point(|start| *start <= start_byte) - 1;
        for (line, line_start) in line_starts[first_line..].iter().enumerate() {
            let line = first_line + line;
            if line >= lines.len() {
                break; // range ended inside the trailing separator
            }
            if *line_start >= end_byte {
                break;
            }
            let line_end = line_start + lines[line].len();
            let from = start_byte.max(*line_start);
            let to = end_byte.min(line_end);
            if from >= to {
                continue;
            }
            let line_text = &lines[line];
            let char_start = line_text[..from - line_start].chars().count();
            let char_end =
                char_start + line_text[from - line_start..to - line_start].chars().count();
            captures[line].push(CaptureSpan { start: char_start, end: char_end, name: index });
        }
    }
    HighlightedBlock(Arc::new(Inner { lines, captures, names }))
}

/// Cache capacity: blocks kept parsed (a long session shows far
/// fewer distinct code blocks than this on screen at once).
const CACHE_CAPACITY: usize = 128;

/// The content-keyed block cache (same policy as the pre-convergence
/// engine: message content is immutable, so a dropped entry only
/// costs one re-parse the next time it scrolls into view).
///
/// G-312: at capacity the cache evicts the OLDEST entry (insertion
/// order), not `clear()` — a full wipe dumped 128 parsed blocks for
/// one insert and re-parsed everything scrolling back into view.
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::new()))
}

/// A bounded `(filetype, content) → block` map with oldest-first
/// (FIFO) eviction — G-312's replacement for the full `clear()`.
struct Cache {
    entries: HashMap<(String, String), HighlightedBlock>,
    order: std::collections::VecDeque<(String, String)>,
}

impl Cache {
    fn new() -> Self {
        Self { entries: HashMap::new(), order: std::collections::VecDeque::new() }
    }

    fn get(&self, key: &(String, String)) -> Option<&HighlightedBlock> {
        self.entries.get(key)
    }

    fn insert(&mut self, key: (String, String), block: HighlightedBlock) {
        if !self.entries.contains_key(&key) && self.entries.len() >= CACHE_CAPACITY {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        if !self.order.contains(&key) {
            self.order.push_back(key.clone());
        }
        self.entries.insert(key, block);
    }
}

/// Highlight one code payload for a filetype (fence info string or
/// registry filetype — aliases and `,extra` fields normalized by the
/// oc-syntax vendor registry lookup, the same convention as before
/// the convergence).
///
/// Returns `None` for unknown filetypes (instantly — the offline
/// rule) and for grammars that fail to initialize; callers fall back
/// to plain rendering.
#[must_use]
pub fn highlight(content: &str, filetype: &str) -> Option<HighlightedBlock> {
    // Offline rule first: unknown filetype → None before any locking.
    let entry = oc_syntax::lookup(filetype)?;

    let key = (entry.filetype.to_owned(), content.to_owned());
    let mut cache = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(block) = cache.get(&key) {
        return Some(block.clone());
    }
    ensure_registered();
    let result = get_tree_sitter_client()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .highlight_once(content, entry.filetype);
    let highlights = result.highlights?;
    let block = build(content, &highlights);
    cache.insert(key, block.clone());
    Some(block)
}

// ------------------------------------------------------------------
// The path-shaped helper surface (unchanged signatures)
// ------------------------------------------------------------------

/// Path extension → registry filetype. G-302: the FULL extension
/// table — every mapping from v2's `LANGUAGE_EXTENSIONS`
/// (util/filetype.ts) whose language has a grammar in the oc-syntax
/// registry, whatever the feature set (unregistered targets simply
/// fall through the offline rule in [`highlight`] — plain render, the
/// same as before), plus the port extras the registry itself aliases
/// (`.pyi`, `.hpp`/`.hh`/`.hxx`, `.toml`, `.vim`, `.agda`, `.ru`).
/// v2's collapse rule (`filetype()`:
/// typescriptreact/javascriptreact/javascript → `typescript`) is kept
/// — `.js` highlights through the typescript grammar, as in v2.
///
/// Sorted-ish by extension (linear scan — the table is tiny);
/// [`EXTENSION_TABLE`] is the single source of truth the coverage
/// test walks (every grammar-registered filetype reachable by at
/// least one extension).
#[rustfmt::skip] // one flat table: extension, filetype — scannable
pub const EXTENSION_TABLE: &[(&str, &str)] = &[
    ("agda", "agda"),
    ("bash", "bash"),
    ("c", "c"),
    ("c++", "cpp"),
    ("cc", "cpp"),
    ("cjs", "typescript"),
    ("clj", "clojure"),
    ("cljc", "clojure"),
    ("cljs", "clojure"),
    ("cpp", "cpp"),
    ("cs", "csharp"),
    ("csx", "csharp"),
    ("css", "css"),
    ("cts", "typescript"),
    ("cxx", "cpp"),
    ("diff", "diff"),
    ("edn", "clojure"),
    ("ets", "typescript"),
    ("ex", "elixir"),
    ("exs", "elixir"),
    ("fsscript", "fsharp"),
    ("fs", "fsharp"),
    ("fsi", "fsharp"),
    ("fsx", "fsharp"),
    ("gemspec", "ruby"),
    ("go", "go"),
    ("h", "c"),
    ("hcl", "hcl"),
    ("hh", "cpp"),
    ("hpp", "cpp"),
    ("hs", "haskell"),
    ("htm", "html"),
    ("html", "html"),
    ("hxx", "cpp"),
    ("java", "java"),
    ("jl", "julia"),
    ("js", "typescript"), // v2 collapses javascript → typescript
    ("json", "json"),
    ("jsx", "typescript"), // v2 collapses javascriptreact → typescript
    ("ksh", "bash"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("lhs", "haskell"),
    ("lua", "lua"),
    ("makefile", "make"),
    ("markdown", "markdown"),
    ("md", "markdown"),
    ("mjs", "typescript"),
    ("ml", "ocaml"),
    ("mli", "ocaml"),
    ("mts", "typescript"),
    ("nix", "nix"),
    ("patch", "diff"),
    ("php", "php"),
    ("py", "python"),
    ("pyi", "python"),
    ("r", "r"),
    ("rake", "ruby"),
    ("rb", "ruby"),
    ("rs", "rust"),
    ("ru", "ruby"),
    ("scala", "scala"),
    ("sh", "bash"),
    ("swift", "swift"),
    ("tf", "terraform"), // registry alias of hcl
    ("toml", "toml"),
    ("ts", "typescript"),
    ("tsx", "typescript"), // v2 collapses typescriptreact → typescript
    ("vim", "vim"),
    ("vue", "vue"),
    ("xml", "xml"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("zsh", "bash"),
];

/// Resolve a file path's extension to a registry filetype (v2's
/// `filetype()`): ASCII-case-insensitive extension match against
/// [`EXTENSION_TABLE`]; unknown extensions return `None` (plain
/// rendering). The returned token may be a registry alias (`tf` →
/// `terraform` → hcl) — [`highlight`]'s registry lookup normalizes.
#[must_use]
pub fn filetype_for_path(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    EXTENSION_TABLE
        .iter()
        .find(|(ext, _)| *ext == extension)
        .map(|(_, filetype)| *filetype)
}

/// Highlight one code payload by its file path's extension: the
/// parsed block when a grammar is registered, `None` for unknown
/// extensions (plain rendering — never blocks, never panics).
#[must_use]
pub fn highlight_for_path(content: &str, path: &str) -> Option<HighlightedBlock> {
    let filetype = filetype_for_path(path)?;
    highlight(content, filetype)
}

/// Convenience for plain-render fallbacks: the payload's lines as
/// single base-styled spans (what unknown extensions render).
#[must_use]
pub fn plain_lines(content: &str, base: Style) -> Vec<StyledSpan> {
    content
        .lines()
        .map(|line| StyledSpan { text: line.to_owned(), style: base, link: None })
        .collect()
}

/// Clip a line's styled spans at `width` display columns — the styled
/// counterpart of `wrap::take_width` (same cluster-width math), with
/// the budget shared across the line's spans.
#[must_use]
pub fn clip_spans(spans: Vec<StyledSpan>, width: usize) -> Vec<StyledSpan> {
    let mut remaining = width;
    let mut out = Vec::with_capacity(spans.len());
    for span in spans {
        if remaining == 0 {
            break;
        }
        let text = crate::wrap::take_width(&span.text, remaining);
        if text.is_empty() {
            break;
        }
        remaining -= crate::wrap::display_width(&text);
        out.push(StyledSpan { text, style: span.style, link: None });
    }
    out
}

/// The base paint style for one code line in the transcript surfaces:
/// `fg` over the given background.
#[must_use]
pub fn code_base(fg: Color, background: Color) -> Style {
    Style::new().fg(fg).bg(background)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oc_theme::Rgba;

    /// v2's `filetype()` routing: the registered extensions resolve,
    /// the js-family collapses to typescript, unknowns return None.
    #[test]
    fn extensions_route_to_registry_filetypes() {
        assert_eq!(filetype_for_path("src/main.rs"), Some("rust"));
        assert_eq!(filetype_for_path("lib/util.py"), Some("python"));
        assert_eq!(filetype_for_path("app.ts"), Some("typescript"));
        assert_eq!(filetype_for_path("app.tsx"), Some("typescript"));
        assert_eq!(filetype_for_path("app.js"), Some("typescript"), "v2 collapses js → ts");
        assert_eq!(filetype_for_path("main.go"), Some("go"));
        assert_eq!(filetype_for_path("stdio.h"), Some("c"));
        assert_eq!(filetype_for_path("matrix.cpp"), Some("cpp"));
        assert_eq!(filetype_for_path("Point.cs"), Some("csharp"));
        assert_eq!(filetype_for_path("App.java"), Some("java"));
        assert_eq!(filetype_for_path("pkg.json"), Some("json"));
        assert_eq!(filetype_for_path("ci.yaml"), Some("yaml"));
        assert_eq!(filetype_for_path("Cargo.toml"), Some("toml"));
        assert_eq!(filetype_for_path("page.html"), Some("html"));
        assert_eq!(filetype_for_path("style.css"), Some("css"));
        assert_eq!(filetype_for_path("0001.patch"), Some("diff"));
        assert_eq!(filetype_for_path("Main.kt"), Some("kotlin"));
        assert_eq!(filetype_for_path("gem.rb"), Some("ruby"));
        assert_eq!(filetype_for_path("index.php"), Some("php"));
        assert_eq!(filetype_for_path("UI.swift"), Some("swift"));
        // G-302 — the G-08 tail languages route (always registered).
        assert_eq!(filetype_for_path("init.vim"), Some("vim"));
        assert_eq!(filetype_for_path("App.scala"), Some("scala"));
        assert_eq!(filetype_for_path("script.lua"), Some("lua"));
        assert_eq!(filetype_for_path("GNUmakefile.makefile"), Some("make"));
        // Case-insensitive extensions; unknown → None (plain render).
        assert_eq!(filetype_for_path("README.MD"), Some("markdown"), "md routes (G-302)");
        assert_eq!(filetype_for_path("notes.txt"), None);
        assert_eq!(filetype_for_path("no-extension"), None);
        assert_eq!(filetype_for_path(""), None);
    }

    /// G-302: every grammar-registered filetype is reachable through
    /// at least one extension in [`EXTENSION_TABLE`] (directly or via
    /// a registry alias — `tf` → `terraform` → hcl). Documented
    /// exceptions: `javascript` (the v2 collapse ruling — js-family
    /// PATHS highlight through the typescript grammar; the javascript
    /// entry stays reachable through its `js` FENCE alias) and
    /// `markdown-inline` (a fence-only label with no path surface).
    #[test]
    fn every_registered_filetype_is_reachable_by_extension() {
        for entry in oc_syntax::registry() {
            if matches!(entry.filetype, "javascript" | "markdown-inline") {
                continue;
            }
            let reachable = EXTENSION_TABLE
                .iter()
                .any(|(_, filetype)| *filetype == entry.filetype || entry.aliases.contains(filetype));
            assert!(reachable, "{} has no extension routing", entry.filetype);
        }
    }

    /// G-302 spot-checks — the extension classes the audit called out.
    /// The `langs-extra`/`lang-*` grammars register behind cargo
    /// features, so their asserts run only when the grammar is
    /// actually registered (the default gate covers the always-on
    /// tail in `extensions_route_to_registry_filetypes`).
    #[test]
    fn spot_checks_for_the_audit_extension_classes() {
        let registered = |filetype: &str| oc_syntax::lookup(filetype).is_some();
        if registered("markdown") {
            assert_eq!(filetype_for_path("README.md"), Some("markdown"));
            assert_eq!(filetype_for_path("notes.markdown"), Some("markdown"));
        }
        if registered("bash") {
            assert_eq!(filetype_for_path("run.sh"), Some("bash"));
            assert_eq!(filetype_for_path("run.zsh"), Some("bash"));
        }
        if registered("xml") {
            assert_eq!(filetype_for_path("pom.xml"), Some("xml"));
        }
        if registered("elixir") {
            assert_eq!(filetype_for_path("app.ex"), Some("elixir"));
            assert_eq!(filetype_for_path("app.exs"), Some("elixir"));
        }
        if registered("haskell") {
            assert_eq!(filetype_for_path("Main.hs"), Some("haskell"));
        }
        if registered("ocaml") {
            assert_eq!(filetype_for_path("main.ml"), Some("ocaml"));
        }
        if registered("fsharp") {
            assert_eq!(filetype_for_path("App.fs"), Some("fsharp"));
            assert_eq!(filetype_for_path("Script.fsx"), Some("fsharp"));
        }
        if registered("clojure") {
            assert_eq!(filetype_for_path("core.clj"), Some("clojure"));
        }
        if registered("nix") {
            assert_eq!(filetype_for_path("flake.nix"), Some("nix"));
        }
        if registered("r") {
            assert_eq!(filetype_for_path("plot.r"), Some("r"));
        }
        if registered("hcl") {
            assert_eq!(filetype_for_path("main.tf"), Some("terraform"));
            assert_eq!(filetype_for_path("main.hcl"), Some("hcl"));
        }
        if registered("julia") {
            assert_eq!(filetype_for_path("main.jl"), Some("julia"));
        }
    }

    /// The full helper: a rust payload parses and materializes themed
    /// spans whose bg stays pinned; an unknown extension is None.
    #[test]
    fn highlight_for_path_parses_and_pins_background() {
        let theme = crate::theme::ThemeState::v2_dark().styles();
        let block =
            highlight_for_path("// note\nlet msg = \"hi\";\n", "src/main.rs").expect("rust");
        let base = code_base(theme.text.fg.unwrap_or_default(), theme.background);
        let spans = block.line_spans(0, base, &theme.syntax);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "// note");
        assert_ne!(spans[0].style.fg, base.fg, "comment resolves to a theme style");
        assert_eq!(spans[0].style.bg, Some(theme.background), "bg pinned to base");
        assert!(highlight_for_path("x = 1", "data.txt").is_none());
    }

    /// The offline rule + alias/fence-extras normalization, unchanged
    /// by the engine swap.
    #[test]
    fn offline_rule_and_alias_normalization_hold() {
        assert!(highlight("let x = 1;", "brainfuck").is_none());
        assert!(highlight("let x = 1;", "").is_none());
        assert!(highlight("", "rust").is_some(), "empty block still highlights");
        let plain = highlight("let x = 1;", "rust").expect("rust");
        let aliased = highlight("let x = 1;", "rs").expect("rs alias");
        let extras = highlight("let x = 1;", "rust,ignore").expect("fence extras");
        for block in [&plain, &aliased, &extras] {
            assert_eq!(block.line_count(), 1);
        }
        // Same grammar+query → same styled output through the alias.
        let theme = crate::theme::ThemeState::v2_dark().styles();
        let base = Style::new();
        assert_eq!(
            plain.line_spans(0, base, &theme.syntax),
            aliased.line_spans(0, base, &theme.syntax),
        );
    }

    /// The UTF-16 → byte → per-line-char conversion boundary: CJK
    /// (3 bytes / 1 UTF-16 unit) and emoji (4 bytes / 2 units)
    /// inside captured ranges must not shift the run boundaries —
    /// the concatenated span text equals the source line byte-for-
    /// byte and the capture covers exactly its glyphs.
    #[test]
    fn utf16_offsets_convert_exactly_across_cjk_and_emoji() {
        // `中` before the string literal, `🙂` inside it: every
        // width class sits between/inside captured ranges.
        let source = "// 注释 🙂 note\nlet s = \"中🙂\"; // 尾";
        let block = highlight(source, "rust").expect("rust parses");
        let theme = crate::theme::ThemeState::v2_dark().styles();
        let base = Style::new();
        for line in 0..block.line_count() {
            let spans = block.line_spans(line, base, &theme.syntax);
            let text: String = spans.iter().map(|span| span.text.as_str()).collect();
            assert_eq!(
                text,
                source.split('\n').nth(line).expect("line exists"),
                "line {line}: styling never changes glyphs"
            );
        }
        // The comment capture covers the full first line including
        // the wide glyphs at both ends of the interior.
        let first = block.line_spans(0, base, &theme.syntax);
        assert_eq!(first.len(), 1, "one comment run: {first:?}");
        assert_eq!(first[0].text, "// 注释 🙂 note");
        // And the second line's string capture slices mid-line after
        // multi-byte glyphs without bleeding into neighbors.
        let second = block.line_spans(1, base, &theme.syntax);
        let joined: String = second.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(joined, "let s = \"中🙂\"; // 尾");
        assert!(second.iter().any(|span| span.text.contains("中🙂")), "{second:?}");
    }

    /// Registration is idempotent: re-registering the catalog (the
    /// startup hook plus the lazy first-highlight both run in a real
    /// process) keeps highlighting correct.
    #[test]
    fn registration_is_idempotent() {
        register_parsers();
        register_parsers();
        let theme = crate::theme::ThemeState::v2_dark().styles();
        let base = Style::new();
        let one = highlight("let x = 1;", "rust").expect("rust");
        register_parsers();
        let two = highlight("let x = 1;", "rust").expect("rust after re-register");
        assert_eq!(
            one.line_spans(0, base, &theme.syntax),
            two.line_spans(0, base, &theme.syntax),
            "re-registration changes no output"
        );
    }

    /// The content-keyed cache returns the same block for repeat
    /// renders (Arc-identical), as before the convergence.
    #[test]
    fn cache_returns_the_same_block() {
        let first = highlight("let x = 1;", "rs").expect("rs parses");
        let second = highlight("let x = 1;", "rs").expect("cached");
        assert!(Arc::ptr_eq(&first.0, &second.0), "same content hits the cache");
    }

    /// G-312: at capacity the cache evicts the OLDEST entry (insertion
    /// order) instead of `clear()` — the earliest block re-parses
    /// (fresh `Arc`) while the latest entries stay cached.
    #[test]
    fn cache_evicts_oldest_at_capacity() {
        let first = highlight("let a = 0;", "rust").expect("parses");
        // Fill past capacity (twice over — robust even if an earlier
        // test in the same process left entries behind).
        let mut last = None;
        for index in 0..(CACHE_CAPACITY * 2) {
            last = highlight(&format!("let v{index} = {index};"), "rust");
        }
        let last = last.expect("parses");
        let reparsed = highlight("let a = 0;", "rust").expect("re-parses");
        assert!(!Arc::ptr_eq(&first.0, &reparsed.0), "the earliest entry was evicted");
        let again =
            highlight(&format!("let v{} = {};", CACHE_CAPACITY * 2 - 1, CACHE_CAPACITY * 2 - 1), "rust")
                .expect("cached");
        assert!(Arc::ptr_eq(&last.0, &again.0), "the latest entry stayed cached");
    }

    /// Scope resolution through the REAL generated theme rules: exact
    /// match, dotted-suffix fallback, alias normalization, attribute
    /// overlay, base fallback — the semantics carried over verbatim.
    #[test]
    fn scope_resolution_semantics_hold() {
        let red = Rgba::from_ints(255, 0, 0, 255);
        let blue = Rgba::from_ints(0, 0, 255, 255);
        let def = |fg: Rgba, italic: bool| SyntaxStyleDef {
            fg: Some(fg),
            bg: None,
            bold: None,
            italic: Some(italic),
            underline: None,
            dim: None,
        };
        let styles = SyntaxStyles(Arc::new(BTreeMap::from([
            ("comment".into(), def(red, true)),
            ("variable.parameter".into(), def(blue, false)),
            ("variable".into(), def(red, false)),
        ])));
        // Exact wins.
        assert_eq!(
            resolve(&styles, "variable.parameter", Style::new()).fg,
            Some(Color::Rgb(0, 0, 255)),
        );
        // Longest dotted suffix.
        assert_eq!(
            resolve(&styles, "variable.other.member", Style::new()).fg,
            Some(Color::Rgb(255, 0, 0)),
        );
        // Unknown → base.
        let base = Style::new().fg(Color::Rgb(9, 9, 9));
        assert_eq!(resolve(&styles, "embedded", base), base);
        assert_eq!(resolve(&SyntaxStyles::empty(), "comment", base), base);
        // Attributes overlay on base (base bg kept).
        let base = Style::new().bg(Color::Rgb(1, 1, 1));
        let style = resolve(&styles, "comment", base);
        assert_eq!(style.bg, Some(Color::Rgb(1, 1, 1)));
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }
}
