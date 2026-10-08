//! Agent/mode markdown discovery — port of `config/plugin/agent.ts`.
//!
//! `{agent,agents}/**/*.md` files become subagents, `{mode,modes}/*.md` become
//! primary agents (per `Config.Directory` entry). Each file decodes through
//! gray-matter frontmatter (native v2 fields, or legacy v1 frontmatter
//! detected by unknown keys and migrated), and is emitted as a synthetic
//! config document `{agents: {<id>: <agent>}}`.

use std::path::{Path, PathBuf};

use oc_schema::agent::Info as AgentInfo;
use oc_schema::config::{Agent, Document, Entry};
use oc_schema::permission::Ruleset;

use crate::config::markdown;
use crate::config::normalize;
use crate::config::Json;

/// Directories scanned for agent markdown, in discovery order.
pub const SOURCE_DIRECTORIES: [&str; 4] = ["agent", "agents", "mode", "modes"];

/// Native frontmatter keys (`variant` + `ConfigAgent.Info` fields).
const AGENT_KEYS: [&str; 11] = [
    "variant",
    "model",
    "request",
    "system",
    "description",
    "mode",
    "hidden",
    "color",
    "steps",
    "disabled",
    "permissions",
];

/// Permission actions whose resources are filesystem paths (home expansion
/// applies); bash resources are raw shell text and are never rewritten.
pub const PATH_ACTIONS: [&str; 3] = ["external_directory", "read", "edit"];

/// One discovered markdown file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFile {
    /// Directory entry the file was found under.
    pub directory: PathBuf,
    pub filepath: PathBuf,
    /// `{mode,modes}/*.md` sources force `mode: "primary"`.
    pub primary: bool,
}

/// Scans `directory` for `{agent,agents}/**/*.md` then `{mode,modes}/*.md`,
/// each group sorted, symlinks followed.
#[must_use]
pub fn discover(directory: &Path) -> Vec<AgentFile> {
    let mut subagents = Vec::new();
    let mut primary = Vec::new();
    for entry in walkdir::WalkDir::new(directory).follow_links(true) {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = match entry.path().strip_prefix(directory) {
            Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        // The v2 glob `*.md` is case-sensitive; keep the exact suffix check.
        #[allow(clippy::case_sensitive_file_extension_comparisons)]
        if !relative.ends_with(".md") {
            continue;
        }
        if let Some(rest) =
            relative.strip_prefix("agent/").or_else(|| relative.strip_prefix("agents/"))
        {
            if !rest.is_empty() && !rest.starts_with('/') {
                subagents.push(AgentFile {
                    directory: directory.to_path_buf(),
                    filepath: entry.path().to_path_buf(),
                    primary: false,
                });
            }
        } else if let Some(rest) =
            relative.strip_prefix("mode/").or_else(|| relative.strip_prefix("modes/"))
        {
            // `{mode,modes}/*.md` — exactly one path segment.
            if !rest.is_empty() && !rest.contains('/') {
                primary.push(AgentFile {
                    directory: directory.to_path_buf(),
                    filepath: entry.path().to_path_buf(),
                    primary: true,
                });
            }
        }
    }
    subagents.sort_by(|a, b| a.filepath.cmp(&b.filepath));
    primary.sort_by(|a, b| a.filepath.cmp(&b.filepath));
    subagents.extend(primary);
    subagents
}

/// Loads every `Directory` entry's markdown agents as synthetic documents.
#[must_use]
pub fn load_documents(entries: &[Entry]) -> Vec<Document> {
    let mut documents = Vec::new();
    for entry in entries {
        if let Entry::Directory(directory) = entry {
            for file in discover(Path::new(&directory.path)) {
                if let Ok(content) = std::fs::read_to_string(&file.filepath) {
                    if let Some(document) = decode(&file, &content) {
                        documents.push(document);
                    }
                }
            }
        }
    }
    documents
}

/// Matches anything at or under `<root>/{agent,agents,mode,modes}`.
#[must_use]
pub fn is_agent_source(entries: &[Entry], file: &Path) -> bool {
    entries.iter().any(|entry| {
        let Entry::Directory(directory) = entry else {
            return false;
        };
        let root = Path::new(&directory.path);
        SOURCE_DIRECTORIES.iter().any(|name| path_contains(&root.join(name), file))
    })
}

/// `decode`: markdown file → synthetic `Document { agents: {id: agent} }`.
/// Empty files and undecodable frontmatter produce `None`.
#[must_use]
pub fn decode(file: &AgentFile, content: &str) -> Option<Document> {
    if content.is_empty() {
        return None;
    }
    let markdown = markdown::parse_option(content)?;
    let name = agent_name(file)?;
    let body = markdown.content.trim().to_string();

    let legacy = markdown
        .data
        .as_object()
        .is_some_and(|entries| entries.iter().any(|(key, _)| !AGENT_KEYS.contains(&key.as_str())));

    // Join legacy model + variant without sending native fields through
    // migration; structured selections and variant-only configs stay put.
    let data = normalize::join_model_variant_markdown(&markdown.data)
        .unwrap_or_else(|| markdown.data.clone());

    let agent: Agent = if legacy {
        let mut fields = data.clone();
        fields.set("name", Json::String(name.clone()));
        fields.set("prompt", Json::String(body.clone()));
        let v1 = normalize::decode_v1_agent_lenient(&fields);
        let migrated = normalize::migrate_agent_document_pub(&v1);
        serde_json::from_value(serde_json::Value::from(migrated)).ok()?
    } else {
        let mut native = data;
        native.set("system", Json::String(body.clone()));
        serde_json::from_value(serde_json::Value::from(native)).ok()?
    };

    let mut entry = serde_json::to_value(&agent).unwrap_or_default();
    if file.primary {
        entry["mode"] = serde_json::Value::String("primary".to_string());
    }
    let agent: Agent = serde_json::from_value(entry).ok()?;
    let mut info = serde_json::Map::new();
    info.insert("agents".to_string(), serde_json::json!({ name.clone(): agent }));
    let info = serde_json::from_value(serde_json::Value::Object(info)).ok()?;
    Some(Document { info, path: Some(file.filepath.display().to_string()) })
}

/// Agent id: relative path minus the leading `agent|agents|mode|modes/`
/// segment and the trailing `.md`.
fn agent_name(file: &AgentFile) -> Option<String> {
    let relative = file.filepath.strip_prefix(&file.directory).ok()?;
    let relative = relative.to_string_lossy().replace('\\', "/");
    let stripped = relative
        .strip_prefix("agent/")
        .or_else(|| relative.strip_prefix("agents/"))
        .or_else(|| relative.strip_prefix("mode/"))
        .or_else(|| relative.strip_prefix("modes/"))
        .unwrap_or(&relative);
    let stripped = stripped.strip_suffix(".md").unwrap_or(stripped);
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.to_string())
}

fn path_contains(parent: &Path, child: &Path) -> bool {
    child.starts_with(parent) && child != parent
}

/// `expandPermissions`: `~`/`$HOME` expansion applies to resources of path
/// actions only — never bash resources (raw shell text).
#[must_use]
pub fn expand_permissions(rules: &Ruleset, home: &Path) -> Ruleset {
    rules
        .iter()
        .map(|rule| {
            if PATH_ACTIONS.contains(&rule.action.as_str()) {
                oc_schema::permission::Rule {
                    action: rule.action.clone(),
                    resource: expand_home(&rule.resource, home),
                    effect: rule.effect,
                }
            } else {
                rule.clone()
            }
        })
        .collect()
}

fn expand_home(resource: &str, home: &Path) -> String {
    if resource == "~" || resource == "$HOME" {
        return home.display().to_string();
    }
    let relative = resource
        .strip_prefix("~/")
        .or_else(|| resource.strip_prefix("$HOME/"))
        .or_else(|| resource.strip_prefix("$HOME\\"));
    match relative {
        Some(relative) => {
            let home = home.display().to_string();
            let separator = if home.contains('\\') { "\\" } else { "/" };
            format!("{home}{separator}{relative}")
        }
        None => resource.to_string(),
    }
}

/// Editor surface the transform drives; the runtime `Agent` service
/// (built-in registry) implements this over its own state.
pub trait AgentEditor {
    /// Insertion-ordered agent ids (a snapshot; mutation during iteration is
    /// not visible to an in-progress `ids()` walk).
    fn ids(&self) -> Vec<String>;
    fn get(&self, id: &str) -> Option<AgentInfo>;
    /// Creates the agent from defaults (with `base_permissions` appended)
    /// when missing. Returns whether it already existed.
    fn ensure(&mut self, id: &str, base_permissions: &Ruleset) -> bool;
    fn update(&mut self, id: &str, modify: &mut dyn FnMut(&mut AgentInfo));
    fn remove(&mut self, id: &str);
    fn set_default(&mut self, id: &str);
}

/// In-memory editor preserving insertion order (tests; agent-lane adapters).
#[derive(Debug, Clone, Default)]
pub struct MapAgentEditor {
    pub agents: Vec<(String, AgentInfo)>,
    pub default: Option<String>,
}

impl MapAgentEditor {
    #[must_use]
    pub fn with_builtin(id: &str, info: AgentInfo) -> Self {
        let mut editor = Self::default();
        editor.agents.push((id.to_string(), info));
        editor
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&AgentInfo> {
        self.agents.iter().find(|(name, _)| name == id).map(|(_, info)| info)
    }
}

impl AgentEditor for MapAgentEditor {
    fn ids(&self) -> Vec<String> {
        self.agents.iter().map(|(id, _)| id.clone()).collect()
    }

    fn get(&self, id: &str) -> Option<AgentInfo> {
        self.agents.iter().find(|(name, _)| name == id).map(|(_, info)| info.clone())
    }

    fn ensure(&mut self, id: &str, base_permissions: &Ruleset) -> bool {
        if self.agents.iter().any(|(name, _)| name == id) {
            return true;
        }
        let mut info = AgentInfo::default_for(oc_schema::id::AgentId::parse(id));
        info.permissions.extend(base_permissions.iter().cloned());
        self.agents.push((id.to_string(), info));
        false
    }

    fn update(&mut self, id: &str, modify: &mut dyn FnMut(&mut AgentInfo)) {
        if let Some((_, info)) = self.agents.iter_mut().find(|(name, _)| name == id) {
            modify(info);
        }
    }

    fn remove(&mut self, id: &str) {
        self.agents.retain(|(name, _)| name != id);
    }

    fn set_default(&mut self, id: &str) {
        self.default = Some(id.to_string());
    }
}

/// `ConfigAgentPlugin` transform: global permissions append to every agent,
/// `default_agent` comes from the latest document, disabled agents are
/// removed, and configured fields overlay with permissions appended after.
pub fn transform(documents: &[Document], home: &Path, editor: &mut dyn AgentEditor) {
    let mut permissions: Ruleset = Vec::new();
    for document in documents {
        if let Some(rules) = &document.info.permissions {
            permissions.extend(expand_permissions(rules, home));
        }
    }
    let default = documents.iter().rev().find_map(|document| document.info.default_agent.clone());
    if let Some(default) = default {
        editor.set_default(&default);
    }
    // Existing agents (a snapshot) all receive the global permissions.
    for id in editor.ids() {
        editor.update(&id, &mut |agent| agent.permissions.extend(permissions.iter().cloned()));
    }

    for document in documents {
        let Some(agents) = &document.info.agents else {
            continue;
        };
        for (id, item) in agents {
            if item.disabled == Some(true) {
                editor.remove(id);
                continue;
            }
            editor.ensure(id, &permissions);
            let item = item.clone();
            editor.update(id, &mut |agent| {
                if let Some(model) = &item.model {
                    agent.model = Some(oc_schema::model::Ref {
                        provider_id: model.provider_id.clone(),
                        id: model.model.clone(),
                        variant: model.variant.clone(),
                    });
                }
                if let Some(request) = &item.request {
                    if let Some(headers) = &request.headers {
                        agent.request.headers.extend(headers.clone());
                    }
                    if let Some(body) = &request.body {
                        agent.request.body.extend(body.clone());
                    }
                }
                if let Some(system) = &item.system {
                    agent.system = Some(system.clone());
                }
                if let Some(description) = &item.description {
                    agent.description = Some(description.clone());
                }
                if let Some(mode) = item.mode {
                    agent.mode = match mode {
                        oc_schema::config::agent::Mode::Subagent => {
                            oc_schema::agent::Mode::Subagent
                        }
                        oc_schema::config::agent::Mode::Primary => oc_schema::agent::Mode::Primary,
                        oc_schema::config::agent::Mode::All => oc_schema::agent::Mode::All,
                    };
                }
                if let Some(hidden) = item.hidden {
                    agent.hidden = hidden;
                }
                if let Some(color) = &item.color {
                    agent.color = Some(color.0.clone());
                }
                if let Some(steps) = item.steps {
                    agent.steps = Some(steps);
                }
                if let Some(rules) = &item.permissions {
                    agent.permissions.extend(expand_permissions(rules, home));
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)] // deliberate empty-state assertions
    use super::*;
    use std::fs;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("oc-core-config-tests")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn config_agent_md_discovers_nested_and_mode_files() {
        let dir = tempdir("agent-md-discover");
        write(&dir.join("agents").join("reviewer.md"), "Review.");
        write(&dir.join("agents").join("team").join("helper.md"), "Help.");
        write(&dir.join("modes").join("plan.md"), "Plan.");
        write(&dir.join("modes").join("nested").join("deep.md"), "Deep.");
        write(&dir.join("agents").join("notes.txt"), "ignored");
        let files = discover(&dir);
        let names: Vec<String> = files.iter().map(|file| agent_name(file).unwrap()).collect();
        assert_eq!(names, vec!["reviewer", "team/helper", "plan"]);
        assert!(files.iter().find(|f| agent_name(f).as_deref() == Some("plan")).unwrap().primary);
        assert!(
            !files.iter().find(|f| agent_name(f).as_deref() == Some("reviewer")).unwrap().primary
        );
    }

    #[test]
    fn config_agent_md_loads_legacy_frontmatter() {
        let dir = tempdir("agent-md-legacy");
        write(
            &dir.join("agents").join("reviewer.md"),
            "---\nmodel: openrouter/openai/gpt-5\ndescription: Markdown description\ntemperature: 0.5\ntools:\n  write: false\n---\nReview carefully.",
        );
        let documents = load_documents(&[Entry::Directory(oc_schema::config::Directory {
            path: dir.display().to_string(),
        })]);
        assert_eq!(documents.len(), 1);
        let agent = documents[0].info.agents.as_ref().unwrap().get("reviewer").unwrap();
        assert_eq!(agent.model.as_ref().unwrap().model.as_str(), "openai/gpt-5");
        assert_eq!(agent.system.as_deref(), Some("Review carefully."));
        assert_eq!(agent.description.as_deref(), Some("Markdown description"));
        let request = agent.request.as_ref().unwrap();
        assert_eq!(
            request.body.as_ref().and_then(|body| body.get("temperature")),
            Some(&serde_json::json!(0.5))
        );
        // tools.write:false → permission edit deny
        let permissions = agent.permissions.as_ref().unwrap();
        assert!(permissions.iter().any(|rule| rule.action == "edit"
            && rule.resource == "*"
            && rule.effect == oc_schema::permission::Effect::Deny));
    }

    #[test]
    fn config_agent_md_loads_native_frontmatter_and_forces_primary_for_modes() {
        let dir = tempdir("agent-md-native");
        write(
            &dir.join("modes").join("plan.md"),
            "---\nmodel: example/chat\nrequest:\n  headers:\n    x-agent: native\n---\nMake a plan.",
        );
        let documents = load_documents(&[Entry::Directory(oc_schema::config::Directory {
            path: dir.display().to_string(),
        })]);
        let agent = documents[0].info.agents.as_ref().unwrap().get("plan").unwrap();
        assert_eq!(agent.mode, Some(oc_schema::config::agent::Mode::Primary));
        assert_eq!(agent.system.as_deref(), Some("Make a plan."));
        assert_eq!(
            agent
                .request
                .as_ref()
                .and_then(|request| request.headers.as_ref())
                .and_then(|headers| headers.get("x-agent"))
                .map(String::as_str),
            Some("native")
        );
    }

    #[test]
    fn config_agent_md_joins_separate_model_variant() {
        let dir = tempdir("agent-md-variant");
        write(
            &dir.join("agents").join("a.md"),
            "---\nmodel: example/chat\nvariant: high\n---\nBody",
        );
        write(
            &dir.join("agents").join("b.md"),
            "---\nmodel: example/chat#high\nvariant: low\n---\nBody",
        );
        write(
            &dir.join("agents").join("c.md"),
            "---\nmodel:\n  providerID: example\n  model: chat\n  variant: high\n---\nBody",
        );
        write(&dir.join("agents").join("d.md"), "---\nvariant: high\n---\nBody");
        let documents = load_documents(&[Entry::Directory(oc_schema::config::Directory {
            path: dir.display().to_string(),
        })]);
        // One synthetic document per file; merge their agents maps.
        let mut agents = std::collections::BTreeMap::new();
        for document in &documents {
            if let Some(map) = &document.info.agents {
                for (name, agent) in map {
                    agents.insert(name.clone(), agent.clone());
                }
            }
        }
        let selection = |name: &str| {
            agents
                .get(name)
                .unwrap()
                .model
                .clone()
                .map(|model| serde_json::to_value(&model).unwrap())
        };
        assert_eq!(
            selection("a").unwrap(),
            serde_json::json!({"providerID": "example", "model": "chat", "variant": "high"})
        );
        // Embedded variant wins; the separate variant is ignored.
        assert_eq!(
            selection("b").unwrap(),
            serde_json::json!({"providerID": "example", "model": "chat", "variant": "high"})
        );
        assert_eq!(
            selection("c").unwrap(),
            serde_json::json!({"providerID": "example", "model": "chat", "variant": "high"})
        );
        assert!(selection("d").is_none());
    }

    #[test]
    fn config_agent_md_skips_empty_and_undecodable_files() {
        let dir = tempdir("agent-md-skip");
        write(&dir.join("agents").join("empty.md"), "");
        write(&dir.join("agents").join("bad.md"), "---\nsteps: not-a-number\n---\nBody");
        let documents = load_documents(&[Entry::Directory(oc_schema::config::Directory {
            path: dir.display().to_string(),
        })]);
        assert!(documents.is_empty());
    }

    #[test]
    fn config_agent_md_disabled_agent_removed_by_transform() {
        let dir = tempdir("agent-md-disabled");
        write(&dir.join("agents").join("disabled.md"), "---\ndisabled: true\n---\nDisabled");
        let documents = load_documents(&[Entry::Directory(oc_schema::config::Directory {
            path: dir.display().to_string(),
        })]);
        let mut editor = MapAgentEditor::default();
        transform(&documents, Path::new("/home/test"), &mut editor);
        assert!(editor.get("disabled").is_none());
    }

    #[test]
    fn config_agent_md_transform_appends_global_permissions_to_every_agent() {
        let info = serde_json::from_value::<oc_schema::config::Info>(serde_json::json!({
            "permissions": [
                {"action": "bash", "resource": "*", "effect": "ask"},
                {"action": "read", "resource": "*", "effect": "allow"}
            ],
            "agents": {
                "build": {"permissions": [{"action": "bash", "resource": "git *", "effect": "allow"}]},
                "reviewer": {"mode": "subagent", "permissions": [{"action": "edit", "resource": "*", "effect": "deny"}]}
            }
        }))
        .unwrap();
        let document = Document { info, path: None };
        let mut editor = MapAgentEditor::with_builtin(
            "build",
            oc_schema::agent::Info::default_for(oc_schema::id::AgentId::parse("build")),
        );
        transform(&[document], Path::new("/home/test"), &mut editor);
        let build = editor.get("build").unwrap();
        let actions: Vec<(&str, &str, oc_schema::permission::Effect)> = build
            .permissions
            .iter()
            .map(|rule| (rule.action.as_str(), rule.resource.as_str(), rule.effect))
            .collect();
        assert_eq!(
            actions,
            vec![
                ("*", "*", oc_schema::permission::Effect::Allow),
                ("external_directory", "*", oc_schema::permission::Effect::Ask),
                ("read", "*.env", oc_schema::permission::Effect::Ask),
                ("read", "*.env.*", oc_schema::permission::Effect::Ask),
                ("read", "*.env.example", oc_schema::permission::Effect::Allow),
                ("bash", "*", oc_schema::permission::Effect::Ask),
                ("read", "*", oc_schema::permission::Effect::Allow),
                ("bash", "git *", oc_schema::permission::Effect::Allow),
            ]
        );
        let reviewer = editor.get("reviewer").unwrap();
        assert!(reviewer.permissions.iter().any(
            |rule| rule.action == "edit" && rule.effect == oc_schema::permission::Effect::Deny
        ));
    }

    #[test]
    fn config_agent_md_transform_expands_home_only_for_path_actions() {
        let info = serde_json::from_value::<oc_schema::config::Info>(serde_json::json!({
            "permissions": [
                {"action": "external_directory", "resource": "~/p/**", "effect": "allow"},
                {"action": "bash", "resource": "$HOME/private/**", "effect": "deny"}
            ],
            "agents": {
                "build": {
                    "permissions": [
                        {"action": "external_directory", "resource": "$HOME/cache/**", "effect": "deny"},
                        {"action": "read", "resource": "~/.config", "effect": "allow"}
                    ]
                }
            }
        }))
        .unwrap();
        let mut editor = MapAgentEditor::default();
        transform(&[Document { info, path: None }], Path::new("/home/test"), &mut editor);
        let build = editor.get("build").unwrap();
        let resources: Vec<&str> =
            build.permissions.iter().map(|rule| rule.resource.as_str()).collect();
        assert!(resources.contains(&"/home/test/p/**"));
        assert!(resources.contains(&"/home/test/cache/**"));
        assert!(resources.contains(&"/home/test/.config"));
        // bash resources are never expanded
        assert!(resources.contains(&"$HOME/private/**"));
    }

    #[test]
    fn config_agent_md_transform_uses_latest_default_agent() {
        let first = serde_json::from_value::<oc_schema::config::Info>(serde_json::json!({
            "default_agent": "reviewer"
        }))
        .unwrap();
        let second = serde_json::from_value::<oc_schema::config::Info>(serde_json::json!({
            "default_agent": "plan"
        }))
        .unwrap();
        let mut editor = MapAgentEditor::default();
        transform(
            &[Document { info: first, path: None }, Document { info: second, path: None }],
            Path::new("/home/test"),
            &mut editor,
        );
        assert_eq!(editor.default.as_deref(), Some("plan"));
    }

    #[test]
    fn config_agent_md_is_agent_source_scopes_to_source_dirs() {
        let entries =
            vec![Entry::Directory(oc_schema::config::Directory { path: "/repo".to_string() })];
        assert!(is_agent_source(&entries, Path::new("/repo/agents/reviewer.md")));
        assert!(is_agent_source(&entries, Path::new("/repo/modes/plan.md")));
        assert!(!is_agent_source(&entries, Path::new("/repo/commands/review.md")));
        assert!(!is_agent_source(&entries, Path::new("/repo/opencode.json")));
    }
}
