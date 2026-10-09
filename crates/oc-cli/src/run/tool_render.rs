//! Port of the `@opencode/tui/mini/tool` inline renderers used by `run`:
//! `toolInlineInfo` (icon/title/description per tool rule), `toolOutputText`
//! (incl. the read tool's JSON page-envelope unwrap), `canonicalToolName`,
//! `primitiveInputSummary`, and `formatPath` (path-format.ts).

use serde_json::Value;

/// `canonicalToolName` (util/tool-display.ts).
#[must_use]
pub fn canonical_tool_name(name: &str) -> String {
    match name {
        "bash" => "shell".to_owned(),
        "task" => "subagent".to_owned(),
        "apply_patch" => "patch".to_owned(),
        other => other.to_owned(),
    }
}

/// `primitiveInputSummary`.
#[must_use]
pub fn primitive_input_summary(input: &serde_json::Map<String, Value>, omit: &[&str]) -> String {
    let entries: Vec<String> = input
        .iter()
        .filter(|(key, value)| {
            !omit.contains(&key.as_str())
                && matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_))
        })
        .map(|(key, value)| match value {
            Value::String(text) => format!("{key}={text}"),
            Value::Bool(flag) => format!("{key}={flag}"),
            other => format!("{key}={other}"),
        })
        .collect();
    if entries.is_empty() {
        String::new()
    } else {
        format!("[{}]", entries.join(", "))
    }
}

/// `formatPath(input, {base, forwardSlashes: true})`: relative to `base`
/// when inside it, absolute otherwise (v2 path-format.ts; the `home`
/// abbreviation option is not used by the run renderer).
#[must_use]
pub fn format_path(input: Option<&str>, base: &str) -> String {
    let Some(input) = input.filter(|value| !value.is_empty()) else {
        return String::new();
    };
    let absolute = std::path::Path::new(input);
    let absolute = if absolute.is_absolute() {
        absolute.to_path_buf()
    } else {
        std::path::Path::new(base).join(input)
    };
    match pathdiff_from(base, &absolute) {
        Some(relative) if relative == "." => ".".to_owned(),
        Some(relative) if !relative.starts_with("..") => relative.replace('\\', "/"),
        _ => absolute.display().to_string().replace('\\', "/"),
    }
}

fn pathdiff_from(base: &str, target: &std::path::Path) -> Option<String> {
    let base_path = std::path::Path::new(base);
    let base = base_path.canonicalize().unwrap_or_else(|_| base_path.to_path_buf());
    let target = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
    let base_components: Vec<_> = base.components().collect();
    let target_components: Vec<_> = target.components().collect();
    let mut common = 0;
    while common < base_components.len()
        && common < target_components.len()
        && base_components[common] == target_components[common]
    {
        common += 1;
    }
    // `target` outside `base`: the relative form would climb with `..`.
    if common < base_components.len() {
        return None;
    }
    let relative: std::path::PathBuf = target_components[common..].iter().collect();
    if relative.as_os_str().is_empty() {
        Some(".".to_owned())
    } else {
        Some(relative.display().to_string())
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn field<'a>(value: &'a serde_json::Map<String, Value>, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

fn dict_field<'a>(input: &'a serde_json::Map<String, Value>, key: &str) -> &'a Value {
    input.get(key).unwrap_or(&Value::Null)
}

fn count(n: i64, label: &str) -> String {
    format!("{n} {label}{}", if n == 1 { "" } else { "es" })
}

/// `readDisplayText`: read's model content is a JSON page envelope; unwrap
/// the human-facing text.
#[must_use]
pub fn read_display_text(input: &str) -> Option<String> {
    if !input.starts_with('{') {
        return None;
    }
    let parsed: Value = serde_json::from_str(input).ok()?;
    let envelope = parsed.as_object()?;
    if let (Some(content), Some(kind)) = (
        envelope.get("content").and_then(Value::as_str),
        envelope.get("type").and_then(Value::as_str),
    ) {
        if kind == "text-page" || envelope.get("encoding").and_then(Value::as_str) == Some("utf8") {
            return Some(content.to_owned());
        }
    }
    let entries = envelope.get("entries")?.as_array()?;
    Some(
        entries
            .iter()
            .filter_map(|entry| match entry {
                Value::String(path) => Some(path.clone()),
                Value::Object(map) => map.get("path").and_then(Value::as_str).map(str::to_owned),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// `toolOutputText(name, content)`.
#[must_use]
pub fn tool_output_text(name: &str, content: Option<&Vec<Value>>) -> String {
    let Some(content) = content else { return String::new() };
    let canonical = canonical_tool_name(name);
    let text_of = |item: &Value| {
        item.as_object()
            .filter(|object| object.get("type").and_then(Value::as_str) == Some("text"))
            .and_then(|object| object.get("text"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    if canonical == "shell" {
        return content.iter().find_map(text_of).unwrap_or_default();
    }
    let joined = content.iter().filter_map(text_of).collect::<Vec<_>>().join("\n");
    if canonical == "read" {
        read_display_text(&joined).unwrap_or(joined)
    } else {
        joined
    }
}

/// `ToolInline` (tool.public.ts).
pub struct ToolInline {
    pub icon: String,
    pub title: String,
    pub description: Option<String>,
    /// `Some("block")` renders icon/title/body in a block; `None` is inline.
    pub block: bool,
    pub body: Option<String>,
}

/// `normalizeInput` (mini/tool.ts): legacy provider shapes alias the path
/// under `filePath` and the subagent under `subagent_type`; the path tools
/// (`read`/`write`/`edit`/`lsp`) and `subagent` read through the alias so
/// old providers do not render "Read " / "Unknown Agent".
fn normalize_input(
    name: &str,
    input: &serde_json::Map<String, Value>,
) -> serde_json::Map<String, Value> {
    let mut normalized = input.clone();
    // v2: `typeof input.path === "string" ? input.path : text(input.filePath)`
    // — a string `path` (even empty) wins over the alias; the override only
    // lands when truthy.
    let aliased = |key: &str, alias: &str| {
        match input.get(key) {
            Some(Value::String(value)) => Some(value.clone()),
            _ => input.get(alias).and_then(Value::as_str).map(str::to_owned),
        }
        .filter(|value| !value.is_empty())
    };
    if matches!(name, "read" | "write" | "edit" | "lsp") {
        if let Some(path) = aliased("path", "filePath") {
            normalized.insert("path".to_owned(), Value::String(path));
        }
    }
    if name == "subagent" {
        if let Some(agent) = aliased("agent", "subagent_type") {
            normalized.insert("agent".to_owned(), Value::String(agent));
        }
    }
    normalized
}

/// `toolInlineInfo(part, directory)` — the per-tool icon/title rules.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn tool_inline_info(
    tool: &str,
    input: &serde_json::Map<String, Value>,
    metadata: &serde_json::Map<String, Value>,
    status: &str,
    output: &str,
    directory: &str,
) -> ToolInline {
    let name = canonical_tool_name(tool);
    let normalized;
    let input = if matches!(name.as_str(), "read" | "write" | "edit" | "lsp" | "subagent") {
        normalized = normalize_input(&name, input);
        &normalized
    } else {
        input
    };
    let path_of = |value: &Value| format_path(Some(&text(value)), directory);
    match name.as_str() {
        "shell" => ToolInline {
            icon: "$".to_owned(),
            title: text(dict_field(input, "command")),
            description: None,
            block: true,
            body: (status == "completed")
                .then(|| output.trim().to_owned())
                .filter(|body| !body.is_empty()),
        },
        "write" => ToolInline {
            icon: "←".to_owned(),
            title: format!("Write {}", path_of(dict_field(input, "path"))),
            description: None,
            block: true,
            body: (status == "completed")
                .then(|| output.to_owned())
                .filter(|body| !body.trim().is_empty()),
        },
        "edit" => {
            let files = metadata.get("files").and_then(Value::as_array);
            let patch = files
                .and_then(|files| files.first())
                .and_then(|file| file.get("patch"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| metadata.get("diff").and_then(Value::as_str).map(str::to_owned));
            ToolInline {
                icon: "←".to_owned(),
                title: format!("Edit {}", path_of(dict_field(input, "path"))),
                description: None,
                block: true,
                body: patch.filter(|body| !body.trim().is_empty()),
            }
        }
        "patch" => {
            let files = metadata.get("files").and_then(Value::as_array).map_or(0usize, Vec::len);
            ToolInline {
                icon: "%".to_owned(),
                title: if files == 0 {
                    "Patch".to_owned()
                } else {
                    format!("Patch {files} file{}", if files == 1 { "" } else { "s" })
                },
                description: None,
                block: false,
                body: None,
            }
        }
        "batch" => {
            let calls = input.get("tool_calls").and_then(Value::as_array).map_or(0usize, Vec::len);
            ToolInline {
                icon: "#".to_owned(),
                title: if calls > 0 {
                    format!("Batch {calls} tool{}", if calls == 1 { "" } else { "s" })
                } else {
                    "Batch".to_owned()
                },
                description: None,
                block: true,
                body: (status == "completed")
                    .then(|| output.to_owned())
                    .filter(|body| !body.trim().is_empty()),
            }
        }
        "subagent" => {
            let agent = text(dict_field(input, "agent"));
            let kind = titlecase(&agent);
            let description = text(dict_field(input, "description"));
            let icon = match status {
                "error" => "✗",
                "running" => "•",
                _ => "✓",
            };
            ToolInline {
                icon: icon.to_owned(),
                title: if description.is_empty() {
                    format!("{kind} Subagent")
                } else {
                    description.clone()
                },
                description: (!description.is_empty()).then(|| format!("{kind} Agent")),
                block: false,
                body: None,
            }
        }
        "question" => {
            let total = input.get("questions").and_then(Value::as_array).map_or(0usize, Vec::len);
            ToolInline {
                icon: "→".to_owned(),
                title: format!("Asked {total} question{}", if total == 1 { "" } else { "s" }),
                description: None,
                block: false,
                body: None,
            }
        }
        "read" => {
            let summary = primitive_input_summary(input, &["path"]);
            ToolInline {
                icon: "→".to_owned(),
                title: format!("Read {}", path_of(dict_field(input, "path"))),
                description: (!summary.is_empty()).then_some(summary),
                block: false,
                body: None,
            }
        }
        "glob" => {
            let root = text(dict_field(input, "path"));
            let title = format!("Glob \"{}\"", text(dict_field(input, "pattern")));
            let suffix = if root.is_empty() {
                String::new()
            } else {
                format!("in {}", format_path(Some(&root), directory))
            };
            let matches = metadata.get("count").and_then(Value::as_i64);
            let description = match matches {
                None => suffix,
                Some(matches) if suffix.is_empty() => count(matches, "match"),
                Some(matches) => format!("{suffix} · {}", count(matches, "match")),
            };
            ToolInline {
                icon: "✱".to_owned(),
                title,
                description: (!description.is_empty()).then_some(description),
                block: false,
                body: None,
            }
        }
        "grep" => {
            let root = text(dict_field(input, "path"));
            let title = format!("Grep \"{}\"", text(dict_field(input, "pattern")));
            let suffix = if root.is_empty() {
                String::new()
            } else {
                format!("in {}", format_path(Some(&root), directory))
            };
            let matches = metadata.get("matches").and_then(Value::as_i64);
            let description = match matches {
                None => suffix,
                Some(matches) if suffix.is_empty() => count(matches, "match"),
                Some(matches) => format!("{suffix} · {}", count(matches, "match")),
            };
            ToolInline {
                icon: "✱".to_owned(),
                title,
                description: (!description.is_empty()).then_some(description),
                block: false,
                body: None,
            }
        }
        "list" => {
            let dir = text(dict_field(input, "path"));
            ToolInline {
                icon: "→".to_owned(),
                title: if dir.is_empty() {
                    "List".to_owned()
                } else {
                    format!("List {}", format_path(Some(&dir), directory))
                },
                description: None,
                block: false,
                body: None,
            }
        }
        "lsp" => {
            let operation = {
                let op = text(dict_field(input, "operation"));
                if op.is_empty() {
                    "request".to_owned()
                } else {
                    op
                }
            };
            let file = text(dict_field(input, "path"));
            let line = dict_field(input, "line").as_i64();
            let character = dict_field(input, "character").as_i64();
            let position = match (line, character) {
                (Some(line), Some(character)) => format!(":{line}:{character}"),
                _ => String::new(),
            };
            ToolInline {
                icon: "→".to_owned(),
                title: if file.is_empty() {
                    format!("LSP {operation}")
                } else {
                    format!("LSP {operation} {}{position}", format_path(Some(&file), directory))
                },
                description: None,
                block: false,
                body: None,
            }
        }
        "webfetch" => {
            let url = text(dict_field(input, "url"));
            ToolInline {
                icon: "%".to_owned(),
                title: if url.is_empty() {
                    "WebFetch".to_owned()
                } else {
                    format!("WebFetch {url}")
                },
                description: None,
                block: false,
                body: None,
            }
        }
        "websearch" => {
            let provider = web_search_provider_label(field(metadata, "provider"));
            let query = text(dict_field(input, "query"));
            ToolInline {
                icon: "◈".to_owned(),
                title: if query.is_empty() {
                    provider.clone()
                } else {
                    format!("{provider} \"{query}\"")
                },
                description: None,
                block: false,
                body: None,
            }
        }
        "skill" => {
            let name = metadata
                .get("name")
                .and_then(Value::as_str)
                .map_or_else(|| text(dict_field(input, "id")), str::to_owned);
            ToolInline {
                icon: "→".to_owned(),
                title: format!("Skill \"{name}\""),
                description: None,
                block: false,
                body: None,
            }
        }
        "invalid" => ToolInline {
            icon: "✗".to_owned(),
            title: "Invalid Tool".to_owned(),
            description: None,
            block: true,
            body: (status == "completed")
                .then(|| output.to_owned())
                .filter(|body| !body.trim().is_empty()),
        },
        _ => {
            // fallbackInline
            let title = if input.is_empty() {
                "Unknown".to_owned()
            } else {
                serde_json::to_string(input).unwrap_or_else(|_| "Unknown".to_owned())
            };
            ToolInline {
                icon: "⚙".to_owned(),
                title: format!("{name} {title}"),
                description: None,
                block: false,
                body: None,
            }
        }
    }
}

fn titlecase(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Unknown".to_owned(),
    }
}

fn web_search_provider_label(provider: &Value) -> String {
    let Some(name) = provider.as_str().filter(|name| !name.is_empty()) else {
        return "Web Search".to_owned();
    };
    if name == "opencode" {
        return "Web Search via OpenCode".to_owned();
    }
    let mut chars = name.chars();
    let capitalized = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    format!("Web Search via {capitalized}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_names() {
        assert_eq!(canonical_tool_name("bash"), "shell");
        assert_eq!(canonical_tool_name("task"), "subagent");
        assert_eq!(canonical_tool_name("apply_patch"), "patch");
        assert_eq!(canonical_tool_name("read"), "read");
    }

    #[test]
    fn read_output_unwraps_page_envelope() {
        let content = vec![json!({
            "type": "text",
            "text": "{\"type\":\"text-page\",\"encoding\":\"utf8\",\"content\":\"hello world\"}"
        })];
        assert_eq!(tool_output_text("read", Some(&content)), "hello world");
    }

    #[test]
    fn shell_output_takes_first_text_only() {
        let content = vec![
            json!({"type": "text", "text": "visible"}),
            json!({"type": "text", "text": "model-only status"}),
        ];
        assert_eq!(tool_output_text("bash", Some(&content)), "visible");
    }

    #[test]
    fn inline_info_shapes() {
        let input = json!({"command": "ls -la"}).as_object().unwrap().clone();
        let info =
            tool_inline_info("bash", &input, &serde_json::Map::new(), "completed", "out", "/w");
        assert_eq!(info.icon, "$");
        assert_eq!(info.title, "ls -la");
        assert!(info.block);
        assert_eq!(info.body.as_deref(), Some("out"));

        let input = json!({"path": "/w/src/main.rs"}).as_object().unwrap().clone();
        let info = tool_inline_info("read", &input, &serde_json::Map::new(), "completed", "", "/w");
        assert_eq!(info.title, "Read src/main.rs");

        let input = json!({"pattern": "*.rs", "path": "/w/src"}).as_object().unwrap().clone();
        let metadata = json!({"count": 3}).as_object().unwrap().clone();
        let info = tool_inline_info("glob", &input, &metadata, "completed", "", "/w");
        assert_eq!(info.title, "Glob \"*.rs\"");
        assert_eq!(info.description.as_deref(), Some("in src · 3 matches"));
    }

    #[test]
    fn normalize_input_aliases_legacy_shapes() {
        // filePath → path for the path tools (v2 normalizeInput).
        let input = json!({"filePath": "/w/src/main.rs"}).as_object().unwrap().clone();
        let info = tool_inline_info("read", &input, &serde_json::Map::new(), "completed", "", "/w");
        assert_eq!(info.title, "Read src/main.rs", "read renders through the filePath alias");

        let input = json!({"filePath": "/w/a.txt"}).as_object().unwrap().clone();
        let info =
            tool_inline_info("write", &input, &serde_json::Map::new(), "completed", "", "/w");
        assert_eq!(info.title, "Write a.txt");

        // subagent_type → agent.
        let input = json!({"subagent_type": "general", "description": "explore the tree"})
            .as_object()
            .unwrap()
            .clone();
        let info = tool_inline_info("task", &input, &serde_json::Map::new(), "completed", "", "/w");
        assert_eq!(info.description.as_deref(), Some("General Agent"));
        assert_eq!(info.title, "explore the tree");

        // An explicit path/agent wins over the alias.
        let input =
            json!({"path": "/w/first.rs", "filePath": "/w/second.rs"}).as_object().unwrap().clone();
        let info = tool_inline_info("read", &input, &serde_json::Map::new(), "completed", "", "/w");
        assert_eq!(info.title, "Read first.rs");
    }
}
