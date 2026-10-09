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

/// The config-driven skill table: discovered once per location boot from
/// the loaded config snapshot.
#[derive(Debug, Default)]
pub struct ConfigSkillTable {
    skills: Vec<Skill>,
}

impl ConfigSkillTable {
    /// No skills.
    #[must_use]
    pub fn empty() -> Self {
        Self { skills: Vec::new() }
    }

    /// Discovers skills from a loaded config snapshot for `location`
    /// (directory entries contribute `<dir>/skill` + `<dir>/skills`;
    /// document `skills` entries contribute paths relative to the
    /// location; `~` expands against `home`).
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
        for directory in sources {
            for skill in scan_directory(&directory) {
                by_id.entry(skill.id.as_str().to_string()).or_insert(skill);
            }
        }
        Self { skills: by_id.into_values().collect() }
    }

    /// Every discovered skill.
    #[must_use]
    pub fn list(&self) -> &[Skill] {
        &self.skills
    }

    /// The discovery sources the table loaded from (diagnostics).
    #[must_use]
    pub fn source_keys(&self) -> Vec<String> {
        self.skills
            .iter()
            .map(|skill| SkillSource::Directory { path: skill.path.clone() }.key())
            .collect()
    }
}

impl SkillTable for ConfigSkillTable {
    fn get(&self, id: &str) -> Option<Skill> {
        self.skills.iter().find(|skill| skill.id.as_str() == id).cloned()
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
