//! Config-lane skill discovery — the minimal production [`SkillTable`]
//! (port of `config/plugin/skill.ts` + `skill-file.ts` over the Rust
//! config loader): config documents' `skills` entries and discovered
//! `.opencode` directories contribute skill directories; each directory
//! scans `{*.md,**/SKILL.md}` and parses frontmatter per the v2
//! `SkillFile.parse` rules (id from the path layout, name/description
//! from frontmatter, autoinvoke from `metadata["opencode/autoinvoke"]` /
//! `disable-model-invocation`).
//!
//! URL sources are recognized but not fetched (the discovery download
//! lane is deferred); they contribute no skills yet.

#![allow(
    clippy::must_use_candidate, // ownership snapshots are queries
    clippy::needless_pass_by_value, // constructors consume their parts
    clippy::too_many_lines, // the assembly table mirrors the v2 layer graph
    clippy::match_same_arms,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::option_if_let_else,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args,
    clippy::type_complexity, // hook/executor seams match the lib's
    clippy::large_enum_variant,
    clippy::struct_field_names,
    clippy::unnested_or_patterns,
    clippy::single_match_else,
    clippy::len_without_is_empty,
    clippy::module_name_repetitions,
    clippy::result_large_err,
    clippy::doc_markdown, // TS identifiers in prose stay unquoted
    clippy::format_push_string,
    clippy::trivially_copy_pass_by_ref,
    clippy::significant_drop_tightening, // assembly order is deliberate
    clippy::large_futures, // boot/assemble mirror the v2 layer composition
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
    clippy::doc_lazy_continuation,
    clippy::elidable_lifetime_names,
    clippy::question_mark // the early-return shape mirrors the TS branch
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oc_schema::config::Entry as ConfigEntry;
use oc_schema::id::{SkillId, SkillName};
use oc_schema::skill::{Skill, SkillSource};

use crate::config::{Json, Loaded};
use crate::tool::skill::SkillTable;

/// The v2 built-in skills internal plugin (`plugin/skill.ts`, mounted in
/// `internal.ts` `pre` BEFORE the config skill plugin): two embedded
/// skills — `opencode` and `report` — with the report body extended by a
/// runtime diagnostics snapshot captured at registration.
///
/// The Rust engine has no skill-editor plugin port (all skills enter
/// through [`ConfigSkillTable::discover`], which runs before the graph's
/// plugin supervisor), so the port lands there: built-ins seed the table
/// first and config skills insert unconditionally after — v2's
/// `editor.add` overwrite precedence (config wins on id collision).
pub mod builtin {
    use super::{ConfigEntry, Loaded, Skill, SkillId, SkillName};

    /// `OpencodeContent` — byte-identical to the reference
    /// `plugin/skill/opencode.md`.
    pub const OPENCODE_MD: &str = include_str!("skill/opencode.md");
    /// `ReportContent` — byte-identical to the reference
    /// `plugin/skill/report.md`.
    pub const REPORT_MD: &str = include_str!("skill/report.md");

    pub const OPENCODE_DESCRIPTION: &str = "Use this skill for any question about OpenCode itself, including how OpenCode works, using or configuring it, migrating from V1 to V2, troubleshooting it, developing plugins or integrations, using the OpenCode SDK, clients, server, or API, and contributing to the OpenCode codebase. Also use it for OpenCode agents, commands, skills, tools, permissions, MCP servers, providers, models, themes, keybinds, formatters, the CLI, TUI, desktop app, and web app.";
    const REPORT_DESCRIPTION: &str = "Use when the user wants to report an opencode issue or bug. Collect standard diagnostics, add user-specific reproduction context, and publish the issue with GitHub CLI.";

    /// `app.version` — the workspace (v2 port) version.
    pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
    /// `app.channel` — the build-time `OPENCODE_CHANNEL` define, default
    /// `"local"` (v2 `cli/version.ts`).
    pub const APP_CHANNEL: &str = match option_env!("OPENCODE_CHANNEL") {
        Some(channel) => channel,
        None => "local",
    };

    /// The two embedded skills, in registration order.
    #[must_use]
    pub fn skills(loaded: &Loaded) -> Vec<Skill> {
        vec![opencode_skill(), report_skill(loaded)]
    }

    fn opencode_skill() -> Skill {
        Skill {
            id: SkillId::parse("opencode"),
            name: SkillName::parse("OpenCode"),
            description: Some(OPENCODE_DESCRIPTION.to_owned()),
            auto_invoke: None,
            path: oc_schema::common::AbsolutePath::parse("/builtin/opencode.md"),
            content: OPENCODE_MD.to_owned(),
        }
    }

    fn report_skill(loaded: &Loaded) -> Skill {
        Skill {
            id: SkillId::parse("report"),
            name: SkillName::parse("Report"),
            description: Some(REPORT_DESCRIPTION.to_owned()),
            auto_invoke: None,
            path: oc_schema::common::AbsolutePath::parse("/builtin/report.md"),
            content: report_content_with_diagnostics(loaded),
        }
    }

    /// `reportContentWithDiagnostics`: the reference body, then the
    /// snapshot block with the TS field set verbatim.
    #[must_use]
    pub fn report_content_with_diagnostics(loaded: &Loaded) -> String {
        [
            REPORT_MD.to_owned(),
            String::new(),
            "## Runtime Diagnostics Snapshot".to_owned(),
            String::new(),
            "These values were captured when the built-in report skill was registered. Verify them before publishing.".to_owned(),
            String::new(),
            format!("- opencode version: {APP_VERSION}"),
            format!("- install/channel: {APP_CHANNEL}"),
            format!("- OS: {} {} ({} {})", os_type(), os_release(), platform(), arch()),
            format!("- Terminal: {}", terminal()),
            format!("- Shell: {}", shell()),
            format!("- Active plugins: {}", active_plugins(loaded)),
        ]
        .join("\n")
    }

    /// `os.type()` for the platforms the port targets.
    fn os_type() -> &'static str {
        match std::env::consts::OS {
            "linux" => "Linux",
            "macos" => "Darwin",
            "windows" => "Windows_NT",
            _ => "Unknown",
        }
    }

    /// `os.release()` — the kernel release (procfs on Linux).
    fn os_release() -> String {
        if std::env::consts::OS == "linux" {
            if let Ok(release) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
                return release.trim().to_owned();
            }
        }
        String::new()
    }

    /// `os.platform()`.
    fn platform() -> &'static str {
        std::env::consts::OS
    }

    /// `os.arch()` — Node's arch names.
    fn arch() -> &'static str {
        match std::env::consts::ARCH {
            "x86_64" => "x64",
            "aarch64" => "arm64",
            "x86" => "ia32",
            "arm" => "arm",
            "riscv64" => "riscv64",
            "loongarch64" => "loong64",
            other => other,
        }
    }

    /// The terminal probe: TERM_PROGRAM / TERM / COLORTERM lines, or the
    /// reference's unavailability sentence.
    fn terminal() -> String {
        let probes = [
            ("TERM_PROGRAM", std::env::var("TERM_PROGRAM").ok()),
            ("TERM", std::env::var("TERM").ok()),
            ("COLORTERM", std::env::var("COLORTERM").ok()),
        ];
        let present: Vec<String> = probes
            .into_iter()
            .filter_map(|(name, value)| value.map(|value| format!("{name}={value}")))
            .collect();
        if present.is_empty() {
            "Unavailable: terminal environment variables are not set".to_owned()
        } else {
            present.join(", ")
        }
    }

    /// The shell probe: SHELL (ComSpec on Windows), or the reference's
    /// unavailability sentence.
    fn shell() -> String {
        std::env::var("SHELL")
            .or_else(|_| std::env::var("ComSpec"))
            .or_else(|_| std::env::var("COMSPEC"))
            .unwrap_or_else(|_| "Unavailable: shell environment variable is not set".to_owned())
    }

    /// `configuredPlugins`: every config document `plugins` entry mapped
    /// verbatim (string or `.package`), sorted — remove directives and
    /// duplicates included, exactly like the reference.
    fn active_plugins(loaded: &Loaded) -> String {
        let mut names: Vec<String> = Vec::new();
        for entry in &loaded.entries {
            let ConfigEntry::Document(document) = entry else { continue };
            let Some(plugins) = &document.info.plugins else { continue };
            for plugin in plugins {
                match plugin {
                    oc_schema::config::Plugin::Spec(spec) => names.push(spec.clone()),
                    oc_schema::config::Plugin::Entry { package, .. } => {
                        names.push(package.clone());
                    }
                }
            }
        }
        if names.is_empty() {
            return "None found in config".to_owned();
        }
        names.sort();
        names.join(", ")
    }
}

/// The config-driven skill table: discovered once per location boot from
/// the loaded config snapshot. The table is interior-mutable so the
/// engine's plugin seam (PLUG-D35 — `apply_location_plugins`) can merge
/// `ctx.skill.transform` results AFTER discovery, exactly like v2's
/// plugins run after the config skill plugin and share the same
/// `Skill.Service` editor state.
#[derive(Debug, Default)]
pub struct ConfigSkillTable {
    /// Id-keyed rows (insertion replaces — v2 `editor.add` overwrite).
    skills: parking_lot::RwLock<Vec<Skill>>,
}

impl ConfigSkillTable {
    /// No skills.
    #[must_use]
    pub fn empty() -> Self {
        Self { skills: parking_lot::RwLock::new(Vec::new()) }
    }

    /// Discovers skills from a loaded config snapshot for `location`
    /// (directory entries contribute `<dir>/skill` + `<dir>/skills`;
    /// document `skills` entries contribute paths relative to the
    /// location; `~` expands against `home`). The built-in skills seed
    /// FIRST (the v2 pre-plugin); config skills insert UNCONDITIONALLY —
    /// on an id collision the config skill wins (v2 `editor.add`
    /// overwrite), and later config sources overwrite earlier ones.
    #[must_use]
    pub fn discover(loaded: &Loaded, location: &Path, home: &Path) -> Self {
        let mut sources: Vec<PathBuf> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let add = |path: PathBuf,
                   sources: &mut Vec<PathBuf>,
                   seen: &mut std::collections::BTreeSet<PathBuf>| {
            if seen.insert(path.clone()) {
                sources.push(path);
            }
        };
        for entry in &loaded.entries {
            match entry {
                ConfigEntry::Directory(directory) => {
                    for name in ["skill", "skills"] {
                        add(PathBuf::from(&directory.path).join(name), &mut sources, &mut seen);
                    }
                }
                ConfigEntry::Document(document) => {
                    let Some(items) = document.info.skills.clone() else { continue };
                    for item in items {
                        if item.contains("://") {
                            // URL source: the download lane is deferred.
                            continue;
                        }
                        let expanded = if let Some(rest) = item.strip_prefix("~/") {
                            home.join(rest)
                        } else {
                            PathBuf::from(item)
                        };
                        let resolved =
                            if expanded.is_absolute() { expanded } else { location.join(expanded) };
                        add(resolved, &mut sources, &mut seen);
                    }
                }
            }
        }
        let mut by_id: BTreeMap<String, Skill> = BTreeMap::new();
        // Built-ins first (the v2 SkillPlugin pre-plugin position), then
        // config skills inserted unconditionally: collisions overwrite —
        // config wins, the later source over the earlier (v2 `editor.add`).
        for skill in builtin::skills(loaded) {
            by_id.insert(skill.id.as_str().to_string(), skill);
        }
        for directory in sources {
            for skill in scan_directory(&directory) {
                by_id.insert(skill.id.as_str().to_string(), skill);
            }
        }
        Self { skills: parking_lot::RwLock::new(by_id.into_values().collect()) }
    }

    /// Every discovered skill (snapshot clone — the table is small and
    /// shared behind the engine's `Arc`).
    #[must_use]
    pub fn list(&self) -> Vec<Skill> {
        self.skills.read().clone()
    }

    /// PLUG-D35: merge ONE plugin-registered skill (a `ctx.skill.transform`
    /// `editor.add` materialization). Insert-or-replace by id — plugins
    /// run after the config plugin in v2's activation order, so the
    /// plugin entry wins an id collision.
    pub fn merge_plugin_skill(&self, skill: Skill) {
        let mut skills = self.skills.write();
        match skills.iter_mut().find(|row| row.id == skill.id) {
            Some(row) => *row = skill,
            None => skills.push(skill),
        }
    }

    /// The discovery sources the table loaded from (diagnostics).
    #[must_use]
    pub fn source_keys(&self) -> Vec<String> {
        self.skills
            .read()
            .iter()
            .map(|skill| SkillSource::Directory { path: skill.path.clone() }.key())
            .collect()
    }
}

impl SkillTable for ConfigSkillTable {
    fn get(&self, id: &str) -> Option<Skill> {
        self.skills.read().iter().find(|skill| skill.id.as_str() == id).cloned()
    }
}

/// `fs.scan("{*.md,**/SKILL.md}")` + `SkillFile.parse` per file, sorted.
fn scan_directory(directory: &Path) -> Vec<Skill> {
    let mut files: Vec<PathBuf> = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else { return Vec::new() };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_md = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        if is_md {
            files.push(path);
        }
    }
    // `**/SKILL.md` — one level of subdirectories keeps the minimal scan
    // honest for the `<dir>/<skill>/SKILL.md` layout without a full walk.
    if let Ok(subdirectories) = std::fs::read_dir(directory) {
        for sub in subdirectories.flatten() {
            let skill_md = sub.path().join("SKILL.md");
            if skill_md.is_file() {
                files.push(skill_md);
            }
        }
    }
    files.sort();
    files.into_iter().filter_map(|file| parse_skill_file(directory, &file)).collect()
}

/// `SkillFile.parse` (skill-file.ts): markdown frontmatter + the id
/// layout rule.
fn parse_skill_file(directory: &Path, filepath: &Path) -> Option<Skill> {
    let content = std::fs::read_to_string(filepath).ok()?;
    let markdown = crate::config::markdown::parse_option(&content)?;
    let fields: BTreeMap<&str, &Json> =
        markdown.data.as_object()?.iter().map(|(key, value)| (key.as_str(), value)).collect();
    let name_field = fields.get("name").and_then(|value| value.as_str().map(str::to_string));
    let description =
        fields.get("description").and_then(|value| value.as_str().map(str::to_string));
    let disable_model_invocation =
        fields.get("disable-model-invocation").and_then(|value| boolean_value(value));

    let file_name = filepath.file_name()?.to_string_lossy().into_owned();
    let parent = filepath.parent()?;
    let id = if parent == directory && file_name != "SKILL.md" {
        file_name.strip_suffix(".md").unwrap_or(&file_name).to_string()
    } else {
        parent.file_name()?.to_string_lossy().into_owned()
    };
    let autoinvoke = fields
        .get("metadata")
        .and_then(|metadata| metadata.get("opencode/autoinvoke"))
        .and_then(boolean_value)
        .or_else(|| disable_model_invocation.map(|disabled| !disabled));
    Some(Skill {
        id: SkillId::parse(id.clone()),
        name: SkillName::parse(name_field.unwrap_or(id)),
        description,
        auto_invoke: autoinvoke,
        path: oc_schema::common::AbsolutePath::parse(filepath.to_string_lossy().into_owned()),
        content: markdown.content,
    })
}

/// `booleanValue` (skill-file.ts): booleans, 0/1, and the true/false word
/// spellings.
fn boolean_value(value: &Json) -> Option<bool> {
    match value {
        Json::Bool(value) => Some(*value),
        Json::Number(number) => match number.to_string().as_str() {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        },
        Json::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Some(true),
            "false" | "no" | "off" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}
