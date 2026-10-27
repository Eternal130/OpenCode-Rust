//! Port of `formatter.ts` — the post-write formatting hook point.
//!
//! v2 shells out to configured formatter commands (prettier et al.) matched
//! by extension. The Rust port pins the trait + registry surface and ships a
//! no-op implementation: tools call [`Formatter::file`] after every write,
//! and the real command execution plugs into the same seam later
//! (`TODO(formatter-lane)`).

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

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use oc_schema::config::formatter::Entry as FormatterEntry;
use oc_schema::config::formatter::Formatter as FormatterConfig;

/// One formatter registration (`formatter/builtins.ts Info`, trimmed to the
/// fields the runner consumes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatterInfo {
    pub name: String,
    /// File extensions (with the leading dot) this formatter handles.
    pub extensions: Vec<String>,
    /// The command template; `$FILE` substitutes the target path.
    pub command: Vec<String>,
}

/// The formatter service trait (`Formatter.Interface.file`): returns whether
/// a formatter ran successfully for the file.
pub trait Formatter: Send + Sync {
    /// Register or replace one formatter (`Editor.set`).
    fn set(&self, formatter: FormatterInfo);
    /// Remove one formatter by name (`Editor.remove`).
    fn remove(&self, name: &str);
    /// Run the matching formatter for `filepath`, if any. `true` when a
    /// formatter applied successfully.
    fn file(&self, filepath: &Path) -> bool;
}

/// The no-op implementation: formatters are recorded but never executed —
/// the observable behavior of v2 with no formatter commands available.
#[derive(Debug, Default)]
pub struct NoopFormatter {
    formatters: std::sync::Mutex<BTreeMap<String, FormatterInfo>>,
}

impl NoopFormatter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The registered formatter names (diagnostics/tests).
    pub fn names(&self) -> Vec<String> {
        self.formatters.lock().unwrap_or_else(|p| p.into_inner()).keys().cloned().collect()
    }
}

impl Formatter for NoopFormatter {
    fn set(&self, formatter: FormatterInfo) {
        self.formatters
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(formatter.name.clone(), formatter);
    }

    fn remove(&self, name: &str) {
        self.formatters.lock().unwrap_or_else(|p| p.into_inner()).remove(name);
    }

    fn file(&self, _filepath: &Path) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Built-in toolchain detection + execution (`formatter/builtins.ts`, 26
// formatters + `config/plugin/formatter.ts` config resolution).
//
// Port notes (mechanical divergences, all documented):
// - oc-core has no logging bus, so REF's `Effect.logInfo/logError` on
//   format attempts and failures become silent skips (same control flow).
// - REF `Npm.which` may auto-install the package on a miss; the port only
//   resolves `node_modules/.bin/<bin>` up the directory chain (no network).
// ---------------------------------------------------------------------------

/// The `enabled` arm of a REF `Info`: how the concrete argv is resolved for
/// a formatter (binary discovery + config-file detection).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Enabled {
    /// `executable(...)`: plain PATH (`which`) lookup, fixed argv.
    Which { bin: String, args: Vec<String> },
    /// A config-supplied literal command (`Config.Formatter.Entry.command`)
    /// or a relative script path (pint's `./vendor/bin/pint`).
    Literal(Vec<String>),
    /// prettier/oxfmt: `package.json` dependency (dependencies or
    /// devDependencies) + `node_modules/.bin/<bin>` binary.
    PackageJsonDependency { dependency: String, bin: String, args: Vec<String> },
    /// biome: `biome.json`/`biome.jsonc` config + npm binary.
    Biome,
    /// clang-format: `.clang-format` config + PATH binary.
    ClangFormat,
    /// ruff: PATH binary + `pyproject.toml`(`[tool.ruff]`)/`ruff.toml`/
    /// `.ruff.toml` or a dependency file mentioning `ruff`.
    Ruff,
    /// air: PATH binary + `--help` first line advertises "R language" and
    /// "formatter".
    Air,
    /// uv: PATH binary + `uv format --help` exits 0.
    Uv,
    /// ocamlformat: `.ocamlformat` config + PATH binary.
    Ocamlformat,
    /// pint: `composer.json` `require`/`require-dev` `laravel/pint`.
    Pint,
}

/// One registered formatter (the full REF `Info`, including the detection
/// arm the `FormatterInfo` trim dropped for the no-op registry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Registered {
    name: String,
    extensions: Vec<String>,
    environment: BTreeMap<String, String>,
    enabled: Enabled,
}

/// The 26 built-in formatters, in REF registration order
/// (`formatter/builtins.ts make()` return array). Extensions and argv are
/// byte-for-byte from REF.
#[must_use]
pub(crate) fn builtin_formatters() -> Vec<Registered> {
    use Enabled::Which;
    let which = |bin: &str, args: &[&str]| Which {
        bin: bin.to_string(),
        args: args.iter().map(|s| (*s).to_string()).collect(),
    };
    let exts = |list: &[&str]| list.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    let web = [
        ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts", ".html", ".htm", ".css",
        ".scss", ".sass", ".less", ".vue", ".svelte", ".json", ".jsonc", ".yaml", ".yml", ".toml",
        ".xml", ".md", ".mdx", ".graphql", ".gql",
    ];
    let js = [".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts"];
    let ruby = [".rb", ".rake", ".gemspec", ".ru"];
    let bun_env = || BTreeMap::from([("BUN_BE_BUN".to_string(), "1".to_string())]);
    vec![
        Registered {
            name: "gofmt".into(),
            extensions: exts(&[".go"]),
            environment: BTreeMap::new(),
            enabled: which("gofmt", &["-w", "$FILE"]),
        },
        Registered {
            name: "mix".into(),
            extensions: exts(&[".ex", ".exs", ".eex", ".heex", ".leex", ".neex", ".sface"]),
            environment: BTreeMap::new(),
            enabled: which("mix", &["format", "$FILE"]),
        },
        Registered {
            name: "oxfmt".into(),
            extensions: exts(&js),
            environment: bun_env(),
            enabled: Enabled::PackageJsonDependency {
                dependency: "oxfmt".into(),
                bin: "oxfmt".into(),
                args: vec!["$FILE".into()],
            },
        },
        Registered {
            name: "prettier".into(),
            extensions: exts(&web),
            environment: bun_env(),
            enabled: Enabled::PackageJsonDependency {
                dependency: "prettier".into(),
                bin: "prettier".into(),
                args: vec!["--write".into(), "$FILE".into()],
            },
        },
        Registered {
            name: "biome".into(),
            extensions: exts(&web),
            environment: bun_env(),
            enabled: Enabled::Biome,
        },
        Registered {
            name: "zig".into(),
            extensions: exts(&[".zig", ".zon"]),
            environment: BTreeMap::new(),
            enabled: which("zig", &["fmt", "$FILE"]),
        },
        Registered {
            name: "clang-format".into(),
            extensions: exts(&[
                ".c", ".cc", ".cpp", ".cxx", ".c++", ".h", ".hh", ".hpp", ".hxx", ".h++", ".ino",
                ".C", ".H",
            ]),
            environment: BTreeMap::new(),
            enabled: Enabled::ClangFormat,
        },
        Registered {
            name: "ktlint".into(),
            extensions: exts(&[".kt", ".kts"]),
            environment: BTreeMap::new(),
            enabled: which("ktlint", &["-F", "$FILE"]),
        },
        Registered {
            name: "ruff".into(),
            extensions: exts(&[".py", ".pyi"]),
            environment: BTreeMap::new(),
            enabled: Enabled::Ruff,
        },
        Registered {
            name: "air".into(),
            extensions: exts(&[".R"]),
            environment: BTreeMap::new(),
            enabled: Enabled::Air,
        },
        Registered {
            name: "uv".into(),
            extensions: exts(&[".py", ".pyi"]),
            environment: BTreeMap::new(),
            enabled: Enabled::Uv,
        },
        Registered {
            name: "rubocop".into(),
            extensions: exts(&ruby),
            environment: BTreeMap::new(),
            enabled: which("rubocop", &["--autocorrect", "$FILE"]),
        },
        Registered {
            name: "standardrb".into(),
            extensions: exts(&ruby),
            environment: BTreeMap::new(),
            enabled: which("standardrb", &["--fix", "$FILE"]),
        },
        Registered {
            name: "htmlbeautifier".into(),
            extensions: exts(&[".erb", ".html.erb"]),
            environment: BTreeMap::new(),
            enabled: which("htmlbeautifier", &["$FILE"]),
        },
        Registered {
            name: "dart".into(),
            extensions: exts(&[".dart"]),
            environment: BTreeMap::new(),
            enabled: which("dart", &["format", "$FILE"]),
        },
        Registered {
            name: "ocamlformat".into(),
            extensions: exts(&[".ml", ".mli"]),
            environment: BTreeMap::new(),
            enabled: Enabled::Ocamlformat,
        },
        Registered {
            name: "terraform".into(),
            extensions: exts(&[".tf", ".tfvars"]),
            environment: BTreeMap::new(),
            enabled: which("terraform", &["fmt", "$FILE"]),
        },
        Registered {
            name: "latexindent".into(),
            extensions: exts(&[".tex"]),
            environment: BTreeMap::new(),
            enabled: which("latexindent", &["-w", "-s", "$FILE"]),
        },
        Registered {
            name: "gleam".into(),
            extensions: exts(&[".gleam"]),
            environment: BTreeMap::new(),
            enabled: which("gleam", &["format", "$FILE"]),
        },
        Registered {
            name: "shfmt".into(),
            extensions: exts(&[".sh", ".bash"]),
            environment: BTreeMap::new(),
            enabled: which("shfmt", &["-w", "$FILE"]),
        },
        Registered {
            name: "nixfmt".into(),
            extensions: exts(&[".nix"]),
            environment: BTreeMap::new(),
            enabled: which("nixfmt", &["$FILE"]),
        },
        Registered {
            name: "rustfmt".into(),
            extensions: exts(&[".rs"]),
            environment: BTreeMap::new(),
            enabled: which("rustfmt", &["$FILE"]),
        },
        Registered {
            name: "pint".into(),
            extensions: exts(&[".php"]),
            environment: BTreeMap::new(),
            enabled: Enabled::Pint,
        },
        Registered {
            name: "ormolu".into(),
            extensions: exts(&[".hs"]),
            environment: BTreeMap::new(),
            enabled: which("ormolu", &["-i", "$FILE"]),
        },
        Registered {
            name: "cljfmt".into(),
            extensions: exts(&[".clj", ".cljs", ".cljc", ".edn"]),
            environment: BTreeMap::new(),
            enabled: which("cljfmt", &["fix", "--quiet", "$FILE"]),
        },
        Registered {
            name: "dfmt".into(),
            extensions: exts(&[".d"]),
            environment: BTreeMap::new(),
            enabled: which("dfmt", &["-i", "$FILE"]),
        },
    ]
}

/// The names of the 26 built-ins, in registration order.
#[must_use]
pub fn builtin_formatter_names() -> Vec<String> {
    builtin_formatters().into_iter().map(|item| item.name).collect()
}

/// The formatter service with real detection/execution
/// (`formatter.ts Service` + `builtins.ts make()` + config overrides from
/// `config/plugin/formatter.ts`).
#[derive(Debug, Default)]
pub struct BuiltinsFormatter {
    directory: PathBuf,
    worktree: PathBuf,
    /// Extra binary dirs searched before PATH (REF `global.bin`; the port
    /// leaves this configurable and empty by default).
    bin_dirs: Vec<PathBuf>,
    formatters: std::sync::Mutex<Vec<Registered>>,
    /// Resolved-argv cache (REF `commands` WeakMap: only successful
    /// resolutions are cached; `None` results re-probe next time).
    resolved: std::sync::Mutex<HashMap<String, Vec<String>>>,
}

impl BuiltinsFormatter {
    /// `make(...)` + `builtIns.forEach(editor.set)` — all 26 built-ins
    /// registered for `directory` (bounded upward searches at `worktree`).
    #[must_use]
    pub fn new(directory: PathBuf, worktree: PathBuf, bin_dirs: Vec<PathBuf>) -> Self {
        Self {
            directory,
            worktree,
            bin_dirs,
            formatters: std::sync::Mutex::new(builtin_formatters()),
            resolved: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// The full config-plugin construction (`config/plugin/formatter.ts`):
    /// `documents` reduce exactly like REF `resolve()` — a boolean resets
    /// everything before it, an object merges named entries over the
    /// previous merge. No `formatter` key anywhere → no formatters at all
    /// (REF returns before `editor.set`).
    #[must_use]
    pub fn from_config_documents(
        directory: PathBuf,
        worktree: PathBuf,
        documents: &[oc_schema::config::Document],
    ) -> Self {
        // REF `resolve(entries)`: documents apply lowest → highest priority;
        // a boolean resets everything before it, an object merges each named
        // formatter's fields over the same name from earlier objects (merging
        // starts from `{}` whenever the previous result was not an object).
        enum Resolved {
            Missing,
            All(bool),
            Entries(BTreeMap<String, FormatterEntry>),
        }
        let mut resolved = Resolved::Missing;
        for document in documents {
            let Some(formatter) = document.info.formatter.as_ref() else { continue };
            match formatter {
                FormatterConfig::All(enabled) => resolved = Resolved::All(*enabled),
                FormatterConfig::Entries(entries) => {
                    let map = match &mut resolved {
                        Resolved::Entries(map) => map,
                        other => {
                            *other = Resolved::Entries(BTreeMap::new());
                            let Resolved::Entries(map) = other else { unreachable!() };
                            map
                        }
                    };
                    for (name, entry) in entries {
                        let current = map.entry(name.clone()).or_default();
                        if entry.disabled.is_some() {
                            current.disabled = entry.disabled;
                        }
                        if entry.command.is_some() {
                            current.command = entry.command.clone();
                        }
                        if entry.environment.is_some() {
                            let mut env = current.environment.clone().unwrap_or_default();
                            if let Some(extra) = &entry.environment {
                                env.extend(extra.clone());
                            }
                            current.environment = Some(env);
                        }
                        if entry.extensions.is_some() {
                            current.extensions = entry.extensions.clone();
                        }
                    }
                }
            }
        }
        let formatter = Self::new(directory, worktree, Vec::new());
        match resolved {
            // No document configures `formatter` → REF returns before
            // `editor.set` — nothing is registered (formatting disabled).
            Resolved::Missing | Resolved::All(false) => {
                *formatter.formatters.lock().unwrap_or_else(|p| p.into_inner()) = Vec::new();
            }
            Resolved::All(true) => {}
            Resolved::Entries(entries) => formatter.apply_entries(&entries),
        }
        formatter
    }

    /// Apply per-name config overrides over the freshly-registered built-ins
    /// (`configured === true` short-circuits; an object — including the
    /// `false`-reset → empty-map case — reaches here).
    fn apply_entries(&self, entries: &BTreeMap<String, FormatterEntry>) {
        let builtins = builtin_formatters();
        let mut list: Vec<Registered> = Vec::new();
        for builtin in &builtins {
            let Some(entry) = entries.get(&builtin.name) else {
                list.push(builtin.clone());
                continue;
            };
            if entry.disabled.unwrap_or(false) {
                continue; // editor.remove(name)
            }
            let mut current = builtin.clone();
            if let Some(extensions) = &entry.extensions {
                current.extensions = extensions.clone();
            }
            if let Some(environment) = &entry.environment {
                let mut env = current.environment.clone();
                env.extend(environment.clone());
                current.environment = env;
            }
            if let Some(command) = &entry.command {
                current.enabled = Enabled::Literal(command.clone());
            }
            list.push(current);
        }
        // Config-only formatters (no built-in of that name): a literal
        // command with config extensions; without a command REF registers
        // `enabled: false`, observationally identical to not registering.
        for (name, entry) in entries {
            if builtins.iter().any(|item| item.name == *name) {
                continue;
            }
            let Some(command) = &entry.command else { continue };
            if entry.disabled.unwrap_or(false) {
                continue;
            }
            list.push(Registered {
                name: name.clone(),
                extensions: entry.extensions.clone().unwrap_or_default(),
                environment: entry.environment.clone().unwrap_or_default(),
                enabled: Enabled::Literal(command.clone()),
            });
        }
        *self.formatters.lock().unwrap_or_else(|p| p.into_inner()) = list;
    }

    /// The registered formatter names, in registration order (diagnostics).
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.formatters
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|item| item.name.clone())
            .collect()
    }

    // --- detection helpers (builtins.ts `findUp`/`which`/probes) ---------

    /// `which(cmd)` over the extra bin dirs then `PATH` (util/which.ts).
    fn which(&self, name: &str) -> Option<String> {
        let mut dirs: Vec<PathBuf> = self.bin_dirs.clone();
        if let Some(path) = std::env::var_os("PATH") {
            dirs.extend(std::env::split_paths(&path));
        }
        dirs.iter()
            .map(|dir| dir.join(name))
            .find(|candidate| is_executable_file(candidate))
            .map(|path| path.to_string_lossy().into_owned())
    }

    /// `fs.findUp(target, directory, worktree)`: every match from
    /// `directory` up to and including `worktree` (or the filesystem root
    /// when the worktree is not an ancestor), nearest first.
    fn find_up(&self, name: &str) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut current = Some(self.directory.as_path());
        while let Some(dir) = current {
            let candidate = dir.join(name);
            if candidate.is_file() {
                found.push(candidate);
            }
            if dir == self.worktree {
                break;
            }
            current = dir.parent();
        }
        found
    }

    /// `Npm.which` trimmed to resolution only: the nearest
    /// `node_modules/.bin/<bin>` executable up the chain (no auto-install).
    fn npm_bin(&self, bin: &str) -> Option<String> {
        let mut current = Some(self.directory.as_path());
        while let Some(dir) = current {
            let candidate = dir.join("node_modules").join(".bin").join(bin);
            if is_executable_file(&candidate) {
                return Some(candidate.to_string_lossy().into_owned());
            }
            if dir == self.worktree {
                break;
            }
            current = dir.parent();
        }
        None
    }

    fn read_text(&self, file: &Path) -> String {
        std::fs::read_to_string(file).unwrap_or_default()
    }

    fn resolve(&self, formatter: &Registered) -> Option<Vec<String>> {
        if let Some(cached) =
            self.resolved.lock().unwrap_or_else(|p| p.into_inner()).get(&formatter.name)
        {
            return Some(cached.clone());
        }
        let resolved = self.resolve_uncached(formatter);
        if let Some(command) = &resolved {
            self.resolved
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(formatter.name.clone(), command.clone());
        }
        resolved
    }

    fn resolve_uncached(&self, formatter: &Registered) -> Option<Vec<String>> {
        match &formatter.enabled {
            Enabled::Which { bin, args } => {
                let path = self.which(bin)?;
                let mut command = vec![path];
                command.extend(args.iter().cloned());
                Some(command)
            }
            Enabled::Literal(command) => Some(command.clone()),
            Enabled::PackageJsonDependency { dependency, bin, args } => {
                for file in self.find_up("package.json") {
                    let json = serde_json::from_str::<serde_json::Value>(&self.read_text(&file))
                        .unwrap_or(serde_json::Value::Null);
                    if !has_dependency(&json, dependency) {
                        continue;
                    }
                    if let Some(path) = self.npm_bin(bin) {
                        let mut command = vec![path];
                        command.extend(args.iter().cloned());
                        return Some(command);
                    }
                }
                None
            }
            Enabled::Biome => {
                let configured =
                    self.find_up("biome.json").len() > 0 || self.find_up("biome.jsonc").len() > 0;
                if !configured {
                    return None;
                }
                let path = self.npm_bin("biome")?;
                Some(vec![path, "format".into(), "--write".into(), "$FILE".into()])
            }
            Enabled::ClangFormat => {
                if self.find_up(".clang-format").is_empty() {
                    return None;
                }
                let path = self.which("clang-format")?;
                Some(vec![path, "-i".into(), "$FILE".into()])
            }
            Enabled::Ruff => {
                let bin = self.which("ruff")?;
                for config in ["pyproject.toml", "ruff.toml", ".ruff.toml"] {
                    let found = self.find_up(config);
                    if found.is_empty() {
                        continue;
                    }
                    if config != "pyproject.toml"
                        || self.read_text(&found[0]).contains("[tool.ruff]")
                    {
                        return Some(vec![bin, "format".into(), "$FILE".into()]);
                    }
                }
                for dependency in ["requirements.txt", "pyproject.toml", "Pipfile"] {
                    let found = self.find_up(dependency);
                    if !found.is_empty() && self.read_text(&found[0]).contains("ruff") {
                        return Some(vec![bin, "format".into(), "$FILE".into()]);
                    }
                }
                None
            }
            Enabled::Air => {
                let bin = self.which("air")?;
                let output = probe(&self.directory, &[bin.clone(), "--help".into()])?;
                let first = String::from_utf8_lossy(&output.stdout)
                    .split('\n')
                    .next()
                    .unwrap_or("")
                    .to_string();
                if first.contains("R language") && first.contains("formatter") {
                    Some(vec![bin, "format".into(), "$FILE".into()])
                } else {
                    None
                }
            }
            Enabled::Uv => {
                let bin = self.which("uv")?;
                probe(&self.directory, &[bin.clone(), "format".into(), "--help".into()])?;
                Some(vec![bin, "format".into(), "--".into(), "$FILE".into()])
            }
            Enabled::Ocamlformat => {
                if self.find_up(".ocamlformat").is_empty() {
                    return None;
                }
                let path = self.which("ocamlformat")?;
                Some(vec![path, "-i".into(), "$FILE".into()])
            }
            Enabled::Pint => {
                for file in self.find_up("composer.json") {
                    let json = serde_json::from_str::<serde_json::Value>(&self.read_text(&file))
                        .unwrap_or(serde_json::Value::Null);
                    if has_record_key(&json, "require", "laravel/pint")
                        || has_record_key(&json, "require-dev", "laravel/pint")
                    {
                        return Some(vec!["./vendor/bin/pint".into(), "$FILE".into()]);
                    }
                }
                None
            }
        }
    }
}

impl Formatter for BuiltinsFormatter {
    fn set(&self, formatter: FormatterInfo) {
        // The trait surface is the no-op registry's trim; built-in
        // registration goes through `new`/config. Kept for trait completeness
        // with Literal execution semantics.
        let registered = Registered {
            name: formatter.name,
            extensions: formatter.extensions,
            environment: BTreeMap::new(),
            enabled: Enabled::Literal(formatter.command),
        };
        let mut list = self.formatters.lock().unwrap_or_else(|p| p.into_inner());
        match list.iter().position(|item| item.name == registered.name) {
            Some(index) => list[index] = registered,
            None => list.push(registered),
        }
    }

    fn remove(&self, name: &str) {
        self.formatters.lock().unwrap_or_else(|p| p.into_inner()).retain(|item| item.name != name);
        self.resolved.lock().unwrap_or_else(|p| p.into_inner()).remove(name);
    }

    fn file(&self, filepath: &Path) -> bool {
        let extension = filepath
            .extension()
            .map(|ext| format!(".{}", ext.to_string_lossy()))
            .unwrap_or_default();
        let matching: Vec<Registered> = self
            .formatters
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|item| item.extensions.contains(&extension))
            .cloned()
            .collect();
        // The whole resolve+run loop is blocking filesystem/process work
        // (detection probes — including uncached `--help` subprocess runs
        // — and the formatter exec itself), called from async write/edit/
        // patch handlers: keep it off the async executor (F1).
        run_blocking(|| self.run_matching(matching, filepath))
    }
}

/// Executes `f` off the async executor: when on a multi-thread tokio
/// runtime (the engine's), `block_in_place` moves the worker's other
/// tasks away so the synchronous process spawns inside cannot stall the
/// runtime; outside tokio (plain unit tests, non-tokio callers) it runs
/// directly. Behavior-neutral — no timeout (REF has none).
fn run_blocking<T>(f: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
}

impl BuiltinsFormatter {
    fn run_matching(&self, matching: Vec<Registered>, filepath: &Path) -> bool {
        for formatter in matching {
            let Some(template) = self.resolve(&formatter) else { continue };
            let command: Vec<String> = template
                .iter()
                .map(|arg| arg.replace("$FILE", &filepath.to_string_lossy()))
                .collect();
            let run = std::process::Command::new(&command[0])
                .args(&command[1..])
                .current_dir(&self.directory)
                .envs(&formatter.environment)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            match run {
                Ok(status) if status.success() => return true,
                // Spawn failure / non-zero exit: REF logs and tries the next
                // matching formatter; the port has no log bus → silent skip.
                _ => continue,
            }
        }
        false
    }
}

/// `which`-style executable-file check (unix mode bits; plain file elsewhere).
fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else { return false };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Run a probe command capturing output (builtins.ts `commandOutput`).
fn probe(directory: &Path, command: &[String]) -> Option<std::process::Output> {
    std::process::Command::new(&command[0])
        .args(&command[1..])
        .current_dir(directory)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
}

fn has_dependency(input: &serde_json::Value, dependency: &str) -> bool {
    has_record_key(input, "dependencies", dependency)
        || has_record_key(input, "devDependencies", dependency)
}

fn has_record_key(input: &serde_json::Value, field: &str, key: &str) -> bool {
    input.get(field).is_some_and(|section| section.get(key).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatter_noop_passes_files_through() {
        let formatter = NoopFormatter::new();
        assert!(!formatter.file(Path::new("/tmp/opencode/x.rs")));
        assert!(formatter.names().is_empty());
        formatter.set(FormatterInfo {
            name: "test".into(),
            extensions: vec![".rs".into()],
            command: vec!["rustfmt".into(), "$FILE".into()],
        });
        assert_eq!(formatter.names(), vec!["test".to_string()]);
        // The no-op registry never executes commands.
        assert!(!formatter.file(Path::new("/tmp/opencode/x.rs")));
        formatter.remove("test");
        assert!(formatter.names().is_empty());
    }
}

#[cfg(test)]
mod builtins_tests {
    use super::builtin_formatters;
    use super::BuiltinsFormatter;
    use super::Formatter;
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::Duration;

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir_in("/tmp/opencode").expect("tempdir")
    }

    /// An executable fake tool (shell script; unix-only tests).
    #[cfg(unix)]
    fn fake_bin(dir: &Path, name: &str, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("write script");
        let mut perms = std::fs::metadata(&path).expect("stat").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod");
        path
    }

    /// A project with every detection marker in place plus a bin dir holding
    /// every PATH-resolvable binary (fakes shadow the real PATH because the
    /// formatter searches `bin_dirs` first).
    #[cfg(unix)]
    fn fully_marked_project() -> (tempfile::TempDir, tempfile::TempDir) {
        let project = tempdir();
        let bin = tempdir();
        for name in [
            "gofmt",
            "mix",
            "zig",
            "ktlint",
            "rubocop",
            "standardrb",
            "htmlbeautifier",
            "dart",
            "terraform",
            "latexindent",
            "gleam",
            "shfmt",
            "nixfmt",
            "rustfmt",
            "ormolu",
            "cljfmt",
            "dfmt",
            "clang-format",
            "ocamlformat",
            "ruff",
        ] {
            fake_bin(bin.path(), name, "exit 0");
        }
        // Probe-driven tools: air --help must advertise "R language
        // formatter"; uv must accept `format --help`.
        fake_bin(bin.path(), "air", "echo 'The R language formatter'; exit 0");
        fake_bin(bin.path(), "uv", "exit 0");
        // npm-resolved binaries.
        let npm_bin = project.path().join("node_modules/.bin");
        std::fs::create_dir_all(&npm_bin).expect("bin dir");
        for name in ["prettier", "oxfmt", "biome"] {
            fake_bin(&npm_bin, name, "exit 0");
        }
        std::fs::write(project.path().join(".clang-format"), "").expect("write");
        std::fs::write(project.path().join(".ocamlformat"), "").expect("write");
        std::fs::write(project.path().join("biome.json"), "{}").expect("write");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"devDependencies":{"prettier":"3","oxfmt":"1"}}"#,
        )
        .expect("write");
        std::fs::write(project.path().join("pyproject.toml"), "[tool.ruff]\n").expect("write");
        std::fs::write(
            project.path().join("composer.json"),
            r#"{"require":{"laravel/pint":"^1.0"}}"#,
        )
        .expect("write");
        let vendor = project.path().join("vendor/bin");
        std::fs::create_dir_all(&vendor).expect("vendor");
        fake_bin(&vendor, "pint", "exit 0");
        (project, bin)
    }

    fn resolve_all(formatter: &BuiltinsFormatter) -> Vec<(String, Option<Vec<String>>)> {
        formatter
            .formatters
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|item| (item.name.clone(), formatter.resolve(item)))
            .collect()
    }

    #[test]
    fn builtin_table_has_the_exact_26_formatters_in_ref_order() {
        let names = super::builtin_formatter_names();
        assert_eq!(
            names,
            vec![
                "gofmt",
                "mix",
                "oxfmt",
                "prettier",
                "biome",
                "zig",
                "clang-format",
                "ktlint",
                "ruff",
                "air",
                "uv",
                "rubocop",
                "standardrb",
                "htmlbeautifier",
                "dart",
                "ocamlformat",
                "terraform",
                "latexindent",
                "gleam",
                "shfmt",
                "nixfmt",
                "rustfmt",
                "pint",
                "ormolu",
                "cljfmt",
                "dfmt",
            ]
        );
        for formatter in builtin_formatters() {
            assert!(!formatter.extensions.is_empty(), "{} has extensions", formatter.name);
            assert!(
                formatter.extensions.iter().all(|ext| ext.starts_with('.')),
                "{} extensions carry the leading dot",
                formatter.name
            );
        }
    }

    /// Detection mapping for all 26: with every config marker + binary in
    /// place, each formatter resolves to REF's exact argv template.
    #[test]
    #[cfg(unix)]
    fn detection_resolves_ref_argv_for_all_26() {
        let (project, bin) = fully_marked_project();
        let formatter = BuiltinsFormatter::new(
            project.path().to_path_buf(),
            project.path().to_path_buf(),
            vec![bin.path().to_path_buf()],
        );
        let resolved = resolve_all(&formatter);
        assert_eq!(resolved.len(), 26, "all 26 registered");
        let get = |name: &str| {
            resolved
                .iter()
                .find(|(item, _)| item == name)
                .unwrap_or_else(|| panic!("{name} registered"))
                .1
                .clone()
                .unwrap_or_else(|| panic!("{name} resolves"))
        };
        // Spot the exact argv templates from builtins.ts.
        assert_eq!(
            get("gofmt"),
            vec![
                bin.path().join("gofmt").to_string_lossy().into_owned(),
                "-w".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("mix"),
            vec![
                bin.path().join("mix").to_string_lossy().into_owned(),
                "format".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("oxfmt"),
            vec![
                project.path().join("node_modules/.bin/oxfmt").to_string_lossy().into_owned(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("prettier"),
            vec![
                project.path().join("node_modules/.bin/prettier").to_string_lossy().into_owned(),
                "--write".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("biome"),
            vec![
                project.path().join("node_modules/.bin/biome").to_string_lossy().into_owned(),
                "format".into(),
                "--write".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("zig"),
            vec![
                bin.path().join("zig").to_string_lossy().into_owned(),
                "fmt".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("clang-format"),
            vec![
                bin.path().join("clang-format").to_string_lossy().into_owned(),
                "-i".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("ktlint"),
            vec![
                bin.path().join("ktlint").to_string_lossy().into_owned(),
                "-F".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("ruff"),
            vec![
                bin.path().join("ruff").to_string_lossy().into_owned(),
                "format".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("air"),
            vec![
                bin.path().join("air").to_string_lossy().into_owned(),
                "format".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("uv"),
            vec![
                bin.path().join("uv").to_string_lossy().into_owned(),
                "format".into(),
                "--".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("rubocop"),
            vec![
                bin.path().join("rubocop").to_string_lossy().into_owned(),
                "--autocorrect".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("standardrb"),
            vec![
                bin.path().join("standardrb").to_string_lossy().into_owned(),
                "--fix".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("htmlbeautifier"),
            vec![bin.path().join("htmlbeautifier").to_string_lossy().into_owned(), "$FILE".into()]
        );
        assert_eq!(
            get("dart"),
            vec![
                bin.path().join("dart").to_string_lossy().into_owned(),
                "format".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("ocamlformat"),
            vec![
                bin.path().join("ocamlformat").to_string_lossy().into_owned(),
                "-i".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("terraform"),
            vec![
                bin.path().join("terraform").to_string_lossy().into_owned(),
                "fmt".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("latexindent"),
            vec![
                bin.path().join("latexindent").to_string_lossy().into_owned(),
                "-w".into(),
                "-s".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("gleam"),
            vec![
                bin.path().join("gleam").to_string_lossy().into_owned(),
                "format".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("shfmt"),
            vec![
                bin.path().join("shfmt").to_string_lossy().into_owned(),
                "-w".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("nixfmt"),
            vec![bin.path().join("nixfmt").to_string_lossy().into_owned(), "$FILE".into()]
        );
        assert_eq!(
            get("rustfmt"),
            vec![bin.path().join("rustfmt").to_string_lossy().into_owned(), "$FILE".into()]
        );
        assert_eq!(get("pint"), vec!["./vendor/bin/pint".to_string(), "$FILE".to_string()]);
        assert_eq!(
            get("ormolu"),
            vec![
                bin.path().join("ormolu").to_string_lossy().into_owned(),
                "-i".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("cljfmt"),
            vec![
                bin.path().join("cljfmt").to_string_lossy().into_owned(),
                "fix".into(),
                "--quiet".into(),
                "$FILE".into()
            ]
        );
        assert_eq!(
            get("dfmt"),
            vec![
                bin.path().join("dfmt").to_string_lossy().into_owned(),
                "-i".into(),
                "$FILE".into()
            ]
        );
    }

    /// Marker-driven tools stay disabled without their config file even
    /// when the binary is on PATH (`file` silently skips).
    #[test]
    #[cfg(unix)]
    fn markerless_detection_stays_disabled() {
        let project = tempdir();
        let bin = tempdir();
        fake_bin(bin.path(), "clang-format", "exit 0");
        fake_bin(bin.path(), "ocamlformat", "exit 0");
        let formatter = BuiltinsFormatter::new(
            project.path().to_path_buf(),
            project.path().to_path_buf(),
            vec![bin.path().to_path_buf()],
        );
        assert!(!formatter.file(&project.path().join("main.c")), "no .clang-format → skip");
        assert!(!formatter.file(&project.path().join("main.ml")), "no .ocamlformat → skip");
        assert!(!formatter.file(&project.path().join("main.py")), "no ruff config → skip");
        assert!(!formatter.file(&project.path().join("README")), "no extension → skip");
    }

    /// ruff's pyproject gate: a pyproject.toml without `[tool.ruff]` does
    /// not enable ruff, but a plain `ruff.toml` does.
    #[test]
    #[cfg(unix)]
    fn ruff_requires_tool_section_in_pyproject() {
        let project = tempdir();
        let bin = tempdir();
        fake_bin(bin.path(), "ruff", "exit 0");
        let formatter = |dir: &Path| {
            BuiltinsFormatter::new(
                dir.to_path_buf(),
                dir.to_path_buf(),
                vec![bin.path().to_path_buf()],
            )
        };
        std::fs::write(project.path().join("pyproject.toml"), "[project]\nname = 'x'\n")
            .expect("write");
        assert!(!formatter(project.path()).file(&project.path().join("m.py")));
        std::fs::write(project.path().join("ruff.toml"), "").expect("write");
        assert!(formatter(project.path()).file(&project.path().join("m.py")));
    }

    /// air's `--help` first line must advertise both "R language" and
    /// "formatter" (a same-named unrelated binary stays disabled).
    #[test]
    #[cfg(unix)]
    fn air_probe_requires_r_language_formatter_banner() {
        let project = tempdir();
        let bin = tempdir();
        fake_bin(bin.path(), "air", "echo 'air — a live reloader'; exit 0");
        let formatter = BuiltinsFormatter::new(
            project.path().to_path_buf(),
            project.path().to_path_buf(),
            vec![bin.path().to_path_buf()],
        );
        assert!(!formatter.file(&project.path().join("m.R")));
    }

    /// Real execution through the trait: the fake rustfmt rewrites the file.
    #[test]
    #[cfg(unix)]
    fn file_runs_the_resolved_command_on_the_target() {
        let project = tempdir();
        let bin = tempdir();
        fake_bin(bin.path(), "rustfmt", "printf 'fmt:%s' \"$(cat \"$1\")\" > \"$1\"");
        let formatter = BuiltinsFormatter::new(
            project.path().to_path_buf(),
            project.path().to_path_buf(),
            vec![bin.path().to_path_buf()],
        );
        let target = project.path().join("lib.rs");
        std::fs::write(&target, "fn main(){}").expect("write");
        assert!(formatter.file(&target));
        assert_eq!(std::fs::read_to_string(&target).expect("read"), "fmt:fn main(){}");
    }

    /// A real binary when available: rustfmt from PATH formats Rust source
    /// (skip-guarded when rustfmt is absent).
    #[test]
    #[cfg(unix)]
    fn real_rustfmt_execution_when_available() {
        let which = std::process::Command::new("which").arg("rustfmt").output().expect("which");
        if !which.status.success() {
            eprintln!("skipping: rustfmt not on PATH");
            return;
        }
        let project = tempdir();
        let formatter = BuiltinsFormatter::new(
            project.path().to_path_buf(),
            project.path().to_path_buf(),
            vec![],
        );
        let target = project.path().join("real.rs");
        std::fs::write(&target, "fn main(){let x=1;}\n").expect("write");
        assert!(formatter.file(&target));
        let formatted = std::fs::read_to_string(&target).expect("read");
        assert!(formatted.contains("let x = 1;"), "rustfmt applied: {formatted}");
    }

    /// F1: `file`'s process spawn runs via `block_in_place` on a
    /// multi-thread runtime — the executor keeps making progress while
    /// the formatter subprocess runs.
    #[test]
    #[cfg(unix)]
    fn file_blocking_run_does_not_stall_the_runtime() {
        let project = tempdir();
        let bin = tempdir();
        // A deliberately slow formatter (300 ms) so a stalled executor
        // would be observable.
        fake_bin(bin.path(), "rustfmt", "sleep 0.3; exit 0");
        let formatter = BuiltinsFormatter::new(
            project.path().to_path_buf(),
            project.path().to_path_buf(),
            vec![bin.path().to_path_buf()],
        );
        let target = project.path().join("slow.rs");
        std::fs::write(&target, "fn main(){}").expect("write");

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_time()
            .build()
            .expect("runtime");
        let progressed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&progressed);
        rt.spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let formatted = rt.block_on(async { formatter.file(&target) });
        assert!(formatted, "slow formatter ran");
        // The 50 ms timer tick landed while the 300 ms process ran — the
        // executor was never blocked by `file`.
        assert!(progressed.load(std::sync::atomic::Ordering::SeqCst));
        rt.shutdown_timeout(Duration::from_secs(5));
    }

    /// findUp walks from the file's directory to the worktree boundary.
    #[test]
    #[cfg(unix)]
    fn detection_finds_config_above_the_file_directory() {
        let project = tempdir();
        let bin = tempdir();
        fake_bin(bin.path(), "clang-format", "exit 0");
        std::fs::write(project.path().join(".clang-format"), "").expect("write");
        let nested = project.path().join("a/b");
        std::fs::create_dir_all(&nested).expect("dirs");
        let formatter = BuiltinsFormatter::new(
            nested.clone(),
            project.path().to_path_buf(),
            vec![bin.path().to_path_buf()],
        );
        assert!(formatter.file(&nested.join("m.c")));
    }

    fn document(json: serde_json::Value) -> oc_schema::config::Document {
        serde_json::from_value(serde_json::json!({ "info": json })).expect("document")
    }

    #[test]
    fn config_resolution_matches_ref_plugin_semantics() {
        let dir = tempdir();
        let path = dir.path().to_path_buf();
        let make = |docs: Vec<serde_json::Value>| {
            let documents: Vec<_> = docs.into_iter().map(document).collect();
            BuiltinsFormatter::from_config_documents(path.clone(), path.clone(), &documents)
        };

        // No `formatter` key anywhere → nothing registered.
        assert!(make(vec![serde_json::json!({})]).names().is_empty());
        // `formatter: true` → all 26.
        assert_eq!(make(vec![serde_json::json!({ "formatter": true })]).names().len(), 26);
        // `formatter: false` → nothing.
        assert!(make(vec![serde_json::json!({ "formatter": false })]).names().is_empty());

        // Per-name override: disable rustfmt.
        let names =
            make(vec![serde_json::json!({ "formatter": { "rustfmt": { "disabled": true } } })])
                .names();
        assert_eq!(names.len(), 25);
        assert!(!names.contains(&"rustfmt".to_string()));

        // A boolean resets before an object merges.
        let names = make(vec![
            serde_json::json!({ "formatter": { "rustfmt": { "disabled": true } } }),
            serde_json::json!({ "formatter": true }),
            serde_json::json!({ "formatter": { "shfmt": { "disabled": true } } }),
        ])
        .names();
        assert_eq!(names.len(), 25, "true resets the earlier disable");
        assert!(names.contains(&"rustfmt".to_string()));
        assert!(!names.contains(&"shfmt".to_string()));

        // A custom formatter: literal command + extensions from config.
        let formatter = make(vec![serde_json::json!({
            "formatter": { "mine": { "command": ["myfmt", "$FILE"], "extensions": [".xyz"] } }
        })]);
        assert!(formatter.names().contains(&"mine".to_string()));
    }

    /// A config-supplied literal command executes with $FILE substitution.
    #[test]
    #[cfg(unix)]
    fn config_command_override_executes() {
        let project = tempdir();
        let bin = tempdir();
        fake_bin(bin.path(), "myfmt", "printf 'X%s' \"$(cat \"$1\")\" > \"$1\"");
        let myfmt = bin.path().join("myfmt").to_string_lossy().into_owned();
        let formatter = BuiltinsFormatter::new(
            project.path().to_path_buf(),
            project.path().to_path_buf(),
            vec![bin.path().to_path_buf()],
        );
        let mut entries = std::collections::BTreeMap::new();
        entries.insert(
            "mine".to_string(),
            oc_schema::config::formatter::Entry {
                command: Some(vec![myfmt, "$FILE".into()]),
                extensions: Some(vec![".xyz".into()]),
                ..Default::default()
            },
        );
        formatter.apply_entries(&entries);
        let target = project.path().join("data.xyz");
        std::fs::write(&target, "v").expect("write");
        assert!(formatter.file(&target));
        assert_eq!(std::fs::read_to_string(&target).expect("read"), "Xv");
    }

    /// Environment vars ship with the run (prettier's BUN_BE_BUN=1).
    #[test]
    #[cfg(unix)]
    fn formatter_environment_is_passed_to_the_command() {
        let project = tempdir();
        let bin = tempdir();
        fake_bin(bin.path(), "myfmt2", "echo \"env=$MYVAR\" > \"$1\"");
        let formatter = BuiltinsFormatter::new(
            project.path().to_path_buf(),
            project.path().to_path_buf(),
            vec![bin.path().to_path_buf()],
        );
        let mut registered = super::Registered {
            name: "envprobe".into(),
            extensions: vec![".envp".into()],
            environment: std::collections::BTreeMap::from([(
                "MYVAR".to_string(),
                "set-by-formatter".to_string(),
            )]),
            enabled: super::Enabled::Literal(vec!["myfmt2".into(), "$FILE".into()]),
        };
        registered.enabled = super::Enabled::Literal(vec![
            bin.path().join("myfmt2").to_string_lossy().into_owned(),
            "$FILE".into(),
        ]);
        formatter.formatters.lock().unwrap_or_else(|p| p.into_inner()).push(registered);
        let target = project.path().join("probe.envp");
        std::fs::write(&target, "").expect("write");
        assert!(formatter.file(&target));
        assert_eq!(std::fs::read_to_string(&target).expect("read").trim(), "env=set-by-formatter");
    }
}
