//! Command markdown discovery — port of `config/plugin/command.ts`.
//!
//! `{command,commands}/**/*.md` files become slash commands: each file's
//! gray-matter frontmatter decodes as `Config.Command` (with the trimmed
//! body as the `template`), and the id is the relative path minus the
//! leading `command|commands/` segment and the trailing `.md`. Config
//! `Document` entries (the `commands` map of `opencode.json` & co.) pass
//! through verbatim — the ref's `loadEntry` returns the document's
//! `commands` map as-is (`command.ts:30-34`).
//!
//! Template evaluation (the execute path) ports `evaluateTemplate`
//! (`command.ts:206-252`): `$1..$N` positional substitution (the LAST
//! placeholder absorbs the remaining args), `$ARGUMENTS` = the whole
//! input, a plain template with input appends the input, and `` !`cmd` ``
//! shell blocks interpolate their combined output.

use std::path::{Path, PathBuf};

use oc_schema::config::{Command, Entry};

use crate::config::markdown;

/// Directories scanned for command markdown (ref `sourceDirectories`).
pub const SOURCE_DIRECTORIES: [&str; 2] = ["command", "commands"];

/// One discovered markdown file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandFile {
    /// Directory entry the file was found under.
    pub directory: PathBuf,
    pub filepath: PathBuf,
}

/// Scans `directory` for `{command,commands}/**/*.md`, sorted by path
/// (ref `files.toSorted()` — deterministic registration order).
#[must_use]
pub fn discover(directory: &Path) -> Vec<CommandFile> {
    let mut files = Vec::new();
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
        for name in SOURCE_DIRECTORIES {
            if let Some(rest) = relative.strip_prefix(&format!("{name}/")) {
                if !rest.is_empty() {
                    files.push(CommandFile {
                        directory: directory.to_path_buf(),
                        filepath: entry.path().to_path_buf(),
                    });
                }
                break;
            }
        }
    }
    files.sort_by(|a, b| a.filepath.cmp(&b.filepath));
    files
}

/// Command id: relative path minus the leading `command|commands/`
/// segment and the trailing `.md` (ref `decode`'s name derivation).
#[must_use]
pub fn command_name(file: &CommandFile) -> Option<String> {
    let relative = file.filepath.strip_prefix(&file.directory).ok()?;
    let relative = relative.to_string_lossy().replace('\\', "/");
    let mut stripped = relative.as_str();
    for name in SOURCE_DIRECTORIES {
        if let Some(rest) = stripped.strip_prefix(&format!("{name}/")) {
            stripped = rest;
            break;
        }
    }
    let stripped = stripped.strip_suffix(".md").unwrap_or(stripped);
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.to_string())
}

/// `decode`: markdown file → `(name, Config.Command)`. Empty files and
/// undecodable frontmatter produce `None` (ref `decode` + the
/// `decodeUnknownOption` filter).
#[must_use]
pub fn decode(file: &CommandFile, content: &str) -> Option<(String, Command)> {
    if content.is_empty() {
        return None;
    }
    let markdown = markdown::parse_option(content)?;
    let name = command_name(file)?;
    let mut data = serde_json::to_value(&markdown.data).ok()?;
    let object = data.as_object_mut()?;
    object.insert(
        "template".to_string(),
        serde_json::Value::String(markdown.content.trim().to_string()),
    );
    let command: Command = serde_json::from_value(data).ok()?;
    Some((name, command))
}

/// Loads the command set from the config entries: every `Document`
/// entry's `commands` map verbatim, plus every `Directory` entry's
/// markdown commands. Later entries overwrite earlier ones on name
/// collisions (the ref's `editor.add` Map.set over the document order).
#[must_use]
pub fn load_commands(entries: &[Entry]) -> Vec<(String, Command)> {
    let mut commands: Vec<(String, Command)> = Vec::new();
    let mut insert = |name: String, command: Command| {
        if let Some(slot) = commands.iter_mut().find(|(existing, _)| *existing == name) {
            slot.1 = command;
        } else {
            commands.push((name, command));
        }
    };
    for entry in entries {
        match entry {
            Entry::Document(document) => {
                if let Some(map) = &document.info.commands {
                    for (name, command) in map {
                        insert(name.clone(), command.clone());
                    }
                }
            }
            Entry::Directory(directory) => {
                for file in discover(Path::new(&directory.path)) {
                    if let Ok(content) = std::fs::read_to_string(&file.filepath) {
                        if let Some((name, command)) = decode(&file, &content) {
                            insert(name, command);
                        }
                    }
                }
            }
        }
    }
    commands
}

/// Matches anything at or under `<root>/{command,commands}` (ref
/// `isCommandSource` — directory-level events such as renames carry no
/// per-file paths, so no file-suffix check).
#[must_use]
pub fn is_command_source(entries: &[Entry], file: &Path) -> bool {
    entries.iter().any(|entry| {
        let Entry::Directory(directory) = entry else {
            return false;
        };
        let root = Path::new(&directory.path);
        SOURCE_DIRECTORIES.iter().any(|name| path_contains(&root.join(name), file))
    })
}

fn path_contains(parent: &Path, child: &Path) -> bool {
    child.starts_with(parent) && child != parent
}

// ---------------------------------------------------------------------------
// evaluateTemplate (command.ts:206-252)
// ---------------------------------------------------------------------------

/// Ref `argsRegex`: `[Image N]`, quoted, or bare tokens.
fn parse_arguments(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut rest = input.trim();
    while !rest.is_empty() {
        rest = rest.trim_start();
        if let Some(stripped) = rest.strip_prefix('[') {
            // `[Image N]` — consumed whole (the ref's first alternation).
            if let Some(end) = stripped.find(']') {
                let inner = &stripped[..end];
                if let Some(number) = inner.strip_prefix("Image ") {
                    if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) {
                        args.push(format!("[{inner}]"));
                        rest = &stripped[end + 1..];
                        continue;
                    }
                }
            }
        }
        let quote = rest.chars().next().and_then(|c| match c {
            '"' | '\'' => Some(c),
            _ => None,
        });
        let token = match quote {
            Some(q) => rest[1..]
                .split(q)
                .next()
                .map(|token| (token.to_string(), 2 * q.len_utf8() + token.len())),
            None => rest.split_whitespace().next().map(|token| (token.to_string(), token.len())),
        };
        match token {
            Some((token, consumed)) if consumed > 0 => {
                args.push(token);
                rest = &rest[consumed.min(rest.len())..];
            }
            _ => break,
        }
    }
    args
}

/// Ref `placeholderRegex` = `/\$(\d+)/g`: every `$N` in the template.
fn placeholders(template: &str) -> Vec<usize> {
    let chars: Vec<char> = template.chars().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '$'
            && index + 1 < chars.len()
            && chars[index + 1].is_ascii_digit()
            && chars[index + 1] != '0'
        {
            let mut end = index + 1;
            while end < chars.len() && chars[end].is_ascii_digit() {
                end += 1;
            }
            let digits: String = chars[index + 1..end].iter().collect();
            if let Ok(position) = digits.parse::<usize>() {
                found.push(position);
            }
            index = end;
        } else {
            index += 1;
        }
    }
    found
}

/// Substitutes `$1..$N` (the last placeholder present absorbs the
/// remaining args) then `$ARGUMENTS`; a template with NO placeholders and
/// no `$ARGUMENTS` appends the raw input (ref `evaluateTemplate` minus
/// the `` !`cmd` `` shell blocks — the server-side execution surface
/// applies those separately through the shell select).
#[must_use]
pub fn evaluate_template(template: &str, input: &str) -> String {
    let args = parse_arguments(input);
    let present = placeholders(template);
    let last = present.iter().copied().max().unwrap_or(0);

    let mut expanded = String::with_capacity(template.len());
    let chars: Vec<char> = template.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '$'
            && index + 1 < chars.len()
            && chars[index + 1].is_ascii_digit()
            && chars[index + 1] != '0'
        {
            let mut end = index + 1;
            while end < chars.len() && chars[end].is_ascii_digit() {
                end += 1;
            }
            let digits: String = chars[index + 1..end].iter().collect();
            let Ok(position) = digits.parse::<usize>() else {
                expanded.push(chars[index]);
                index += 1;
                continue;
            };
            let arg_index = position.saturating_sub(1);
            if arg_index < args.len() {
                if position == last {
                    expanded.push_str(&args[arg_index..].join(" "));
                } else {
                    expanded.push_str(&args[arg_index]);
                }
            }
            index = end;
        } else {
            expanded.push(chars[index]);
            index += 1;
        }
    }

    let with_arguments = expanded.replace("$ARGUMENTS", input);
    if present.is_empty() && !template.contains("$ARGUMENTS") && !input.trim().is_empty() {
        format!("{with_arguments}\n\n{input}").trim().to_string()
    } else {
        with_arguments.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("oc-core-config-tests")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn config_command_md_discovers_nested_files() {
        let dir = tempdir("command-md-discover");
        write(&dir.join("command").join("review.md"), "Review it.");
        write(&dir.join("commands").join("team").join("plan.md"), "Plan it.");
        write(&dir.join("command").join("notes.txt"), "ignored");
        write(&dir.join("other").join("x.md"), "ignored");
        let files = discover(&dir);
        let names: Vec<String> = files.iter().filter_map(command_name).collect();
        assert_eq!(names, vec!["review", "team/plan"]);
    }

    #[test]
    fn config_command_md_decodes_frontmatter_and_template() {
        let dir = tempdir("command-md-decode");
        write(
            &dir.join("command").join("issue.md"),
            "---\ndescription: File an issue\nsubagent: true\n---\nFile $1 against $2 please.",
        );
        let files = discover(&dir);
        let (name, command) =
            decode(&files[0], &std::fs::read_to_string(&files[0].filepath).unwrap())
                .expect("decode");
        assert_eq!(name, "issue");
        assert_eq!(command.description.as_deref(), Some("File an issue"));
        assert_eq!(command.subagent, Some(true));
        assert_eq!(command.template, "File $1 against $2 please.");
    }

    #[test]
    fn config_command_md_skips_empty_and_undecodable() {
        let dir = tempdir("command-md-skip");
        write(&dir.join("command").join("empty.md"), "");
        // Frontmatter `template` is intentionally OVERRIDDEN by the body
        // (ref `decode`: `{ ...markdown.data, template: body }`), so a
        // wrong-typed template key alone is NOT undecodable — a wrong
        // typed OTHER field is.
        write(&dir.join("command").join("bad.md"), "---\nsubagent: not-a-bool\n---\nBody");
        write(&dir.join("command").join("typed-template.md"), "---\ntemplate: 5\n---\nBody wins.");
        let commands = load_commands(&[Entry::Directory(oc_schema::config::Directory {
            path: dir.display().to_string(),
        })]);
        assert_eq!(commands.len(), 1, "only the typed-template file decodes: {commands:?}");
        assert_eq!(commands[0].0, "typed-template");
        assert_eq!(commands[0].1.template, "Body wins.");
    }

    #[test]
    fn config_command_md_merges_config_documents_and_markdown() {
        let dir = tempdir("command-md-merge");
        write(&dir.join("command").join("md-cmd.md"), "Markdown command.");
        let info = serde_json::from_value::<oc_schema::config::Info>(serde_json::json!({
            "commands": {"cfg-cmd": {"template": "Config command."}}
        }))
        .unwrap();
        let commands = load_commands(&[
            Entry::Document(Box::new(oc_schema::config::Document { info, path: None })),
            Entry::Directory(oc_schema::config::Directory { path: dir.display().to_string() }),
        ]);
        let names: Vec<&str> = commands.iter().map(|(name, _)| name.as_str()).collect();
        assert!(names.contains(&"cfg-cmd"), "{names:?}");
        assert!(names.contains(&"md-cmd"), "{names:?}");
        // Later documents overwrite on collision (editor.add Map.set).
        let later = serde_json::from_value::<oc_schema::config::Info>(serde_json::json!({
            "commands": {"md-cmd": {"template": "Winner."}}
        }))
        .unwrap();
        let commands = load_commands(&[
            Entry::Directory(oc_schema::config::Directory { path: dir.display().to_string() }),
            Entry::Document(Box::new(oc_schema::config::Document { info: later, path: None })),
        ]);
        let winner = commands.iter().find(|(name, _)| name == "md-cmd").unwrap();
        assert_eq!(winner.1.template, "Winner.");
    }

    #[test]
    fn config_command_md_is_command_source_scopes_to_source_dirs() {
        let entries =
            vec![Entry::Directory(oc_schema::config::Directory { path: "/repo".to_string() })];
        assert!(is_command_source(&entries, Path::new("/repo/command/review.md")));
        assert!(is_command_source(&entries, Path::new("/repo/commands/team/plan.md")));
        assert!(!is_command_source(&entries, Path::new("/repo/agents/reviewer.md")));
        assert!(!is_command_source(&entries, Path::new("/repo/opencode.json")));
    }

    #[test]
    fn config_command_md_template_positional_substitution() {
        // Last placeholder absorbs the rest.
        assert_eq!(evaluate_template("Fix $1 then $2", "a b c"), "Fix a then b c");
        assert_eq!(evaluate_template("Fix $2", "a b c"), "Fix b c");
        // $ARGUMENTS is the whole input.
        assert_eq!(evaluate_template("Do: $ARGUMENTS", "a b"), "Do: a b");
        // Plain template + input appends the input.
        assert_eq!(evaluate_template("Summarize.", "the text"), "Summarize.\n\nthe text");
        // No input: plain template stays.
        assert_eq!(evaluate_template("Summarize.", "  "), "Summarize.");
        // Quoted args and [Image N] tokens stay atomic.
        assert_eq!(evaluate_template("Fix $1", "\"a b\" c"), "Fix a b c");
        assert_eq!(evaluate_template("See $1", "[Image 2] hi"), "See [Image 2] hi");
        assert_eq!(evaluate_template("See $2", "[Image 2] hi"), "See hi");
    }
}
