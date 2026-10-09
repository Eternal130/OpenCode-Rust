//! Port of `packages/core/src/shell/parse.ts` — permission scanning over a
//! command: per-command `{resource, save}` pairs plus touched directories.
//!
//! Two backends, exactly like v2:
//!
//! - **legacy** — the tree-sitter grammars (`tree-sitter-bash`,
//!   `tree-sitter-powershell` crates here): `command` nodes become
//!   resources; declaration nodes fall out naturally.
//! - **portable** — the hand-written [`super::scan`] scanner, selected via
//!   `experimental.portable_shell_scanner`.

use std::path::Path;

use oc_util::wildcard;

use super::scan::{self, Command as ScannedCommand, Dialect};
use super::select;

/// `ShellParse.Result`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanOutput {
    pub commands: Vec<ScannedCommandPermission>,
    pub directories: Vec<String>,
}

/// One command's permission spans.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedCommandPermission {
    pub resource: String,
    pub save: String,
}

/// Commands whose arguments change the working directory.
fn is_cwd_command(name: &str) -> bool {
    matches!(name, "cd" | "chdir" | "popd" | "pushd" | "push-location" | "set-location")
}

/// `PREFIX_LENGTH = max(ARITY)`.
const PREFIX_LENGTH: usize = 3;

/// The ARITY table, verbatim from parse.ts.
#[rustfmt::skip]
fn arity_table() -> &'static [(&'static str, usize)] {
    &[
        ("cat", 1), ("cd", 1), ("chmod", 1), ("chown", 1), ("cp", 1), ("echo", 1), ("env", 1),
        ("export", 1), ("grep", 1), ("kill", 1), ("killall", 1), ("ln", 1), ("ls", 1),
        ("mkdir", 1), ("mv", 1), ("ps", 1), ("pwd", 1), ("rm", 1), ("rmdir", 1), ("sleep", 1),
        ("source", 1), ("tail", 1), ("touch", 1), ("unset", 1), ("which", 1),
        ("aws", 3), ("az", 3), ("bazel", 2), ("brew", 2), ("bun", 2), ("bun run", 3), ("bun x", 3),
        ("cargo", 2), ("cargo add", 3), ("cargo run", 3), ("cdk", 2), ("cf", 2), ("cmake", 2),
        ("composer", 2), ("consul", 2), ("consul kv", 3), ("crictl", 2), ("deno", 2),
        ("deno task", 3), ("doctl", 3), ("docker", 2), ("docker builder", 3), ("docker compose", 3),
        ("docker container", 3), ("docker image", 3), ("docker network", 3), ("docker volume", 3),
        ("eksctl", 2), ("eksctl create", 3), ("firebase", 2), ("flyctl", 2), ("gcloud", 3),
        ("gh", 3), ("git", 2), ("git config", 3), ("git remote", 3), ("git stash", 3), ("go", 2),
        ("gradle", 2), ("helm", 2), ("heroku", 2), ("hugo", 2), ("ip", 2), ("ip addr", 3),
        ("ip link", 3), ("ip netns", 3), ("ip route", 3), ("kind", 2), ("kind create", 3),
        ("kubectl", 2), ("kubectl kustomize", 3), ("kubectl rollout", 3), ("kustomize", 2),
        ("make", 2), ("mc", 2), ("mc admin", 3), ("minikube", 2), ("mongosh", 2), ("mysql", 2),
        ("mvn", 2), ("ng", 2), ("npm", 2), ("npm exec", 3), ("npm init", 3), ("npm run", 3),
        ("npm view", 3), ("npx", 2), ("nvm", 2), ("nx", 2), ("openssl", 2), ("openssl req", 3),
        ("openssl x509", 3), ("pip", 2), ("pipenv", 2), ("pnpm", 2), ("pnpm dlx", 3),
        ("pnpm exec", 3), ("pnpm run", 3), ("poetry", 2), ("podman", 2), ("podman container", 3),
        ("podman image", 3), ("psql", 2), ("pulumi", 2), ("pulumi stack", 3), ("python", 2),
        ("pyenv", 2), ("rake", 2), ("rbenv", 2), ("redis-cli", 2), ("rustup", 2),
        ("serverless", 2), ("sfdx", 3), ("skaffold", 2), ("sls", 2), ("sst", 2), ("swift", 2),
        ("systemctl", 2), ("terraform", 2), ("terraform workspace", 3), ("tmux", 2), ("turbo", 2),
        ("ufw", 2), ("vault", 2), ("vault auth", 3), ("vault kv", 3), ("vercel", 2), ("volta", 2),
        ("wp", 2), ("yarn", 2), ("yarn dlx", 3), ("yarn run", 3),
    ]
}

fn arity(command: &str) -> Option<usize> {
    arity_table().iter().find(|(key, _)| *key == command).map(|(_, value)| *value)
}

/// `prefix(tokens)` — the longest known command head, truncated to its arity.
fn prefix(tokens: &[String]) -> usize {
    for length in (1..=tokens.len()).rev() {
        let candidate = tokens[..length].join(" ");
        if let Some(arity) = arity(&candidate) {
            return arity.min(tokens.len());
        }
    }
    1
}

/// `ShellParse.scan` — dispatch to the legacy (tree-sitter) or portable
/// scanner.
pub fn scan(command: &str, shell: &str, cwd: &Path, portable: bool) -> Result<ScanOutput, String> {
    if portable {
        scan_portable(command, shell, cwd)
    } else {
        scan_legacy(command, shell, cwd)
    }
}

// ---------------------------------------------------------------------------
// Portable scanner
// ---------------------------------------------------------------------------

/// `ShellParse.scanPortable`.
pub fn scan_portable(command: &str, shell: &str, cwd: &Path) -> Result<ScanOutput, String> {
    let powershell = select::ps(shell);
    let result = if powershell {
        scan::scan_powershell(command)
    } else {
        let dialect = match select::name(shell).as_str() {
            "bash" => Dialect::Bash,
            "zsh" => Dialect::Zsh,
            _ => Dialect::Posix,
        };
        scan::scan(command, dialect)
    };
    let commands = match result {
        scan::ScanResult::Scanned { commands } => commands,
        scan::ScanResult::Opaque { reason } => {
            return Err(format!("Portable shell scanner cannot analyze command: {reason}"))
        }
    };

    let mut output = ScanOutput::default();
    for item in commands {
        // The legacy command walk skips declarations, not the substitutions
        // within them.
        if item.declaration {
            continue;
        }
        let words: Vec<String> = match item.redirect_word_count {
            Some(count) => item.raw_words.iter().take(count).cloned().collect(),
            None => item.raw_words.clone(),
        };
        // The shipped PowerShell grammar treats bare statement-head foreach
        // blocks as control flow.
        if powershell
            && item.statement_head
            && words.first().is_some_and(|word| {
                let lower = word.to_lowercase();
                lower == "foreach" || lower.starts_with("foreach-")
            })
            && words.len() > 1
            && words[1..].iter().all(|word| word.starts_with('{'))
        {
            continue;
        }
        let name =
            words.first().map(|word| if powershell { word.to_lowercase() } else { word.clone() });
        if name.as_deref().is_some_and(is_cwd_command) {
            let parts = directory_parts(&item, &words, powershell);
            output.directories.extend(
                directory_args(&parts, powershell, &cwd.to_string_lossy(), shell)
                    .into_iter()
                    .flatten(),
            );
            continue;
        }
        let scanned_head = &words[..words.len().min(PREFIX_LENGTH)];
        let selected_length = prefix(scanned_head).min(words.len());
        let selected = &words[..selected_length];
        let conventional = format!("{} *", selected.join(" "));
        let end = item
            .word_ends
            .as_ref()
            .and_then(|ends| ends.get(selected.len().saturating_sub(1)).copied());
        // Keep existing grants stable unless normalized spacing loses the
        // original source boundary.
        let save = if !powershell
            || end.is_none()
            || wildcard::matches(&item.resource, &conventional)
        {
            conventional
        } else {
            let end = end.unwrap_or(0);
            let boundary = item
                .word_ends
                .as_ref()
                .and_then(|ends| {
                    ends.iter().copied().find(|value| {
                        *value >= end
                            && (*value >= item.resource.chars().count()
                                || item
                                    .resource
                                    .chars()
                                    .nth(*value)
                                    .is_some_and(char::is_whitespace))
                    })
                })
                .unwrap_or(end);
            let separator = separator_after(&item.resource, boundary);
            format!("{}{}*", &item.resource[..char_boundary(&item.resource, boundary)], separator)
        };
        output.commands.push(ScannedCommandPermission { resource: item.resource, save });
    }
    Ok(output)
}

fn char_boundary(text: &str, char_index: usize) -> usize {
    text.char_indices().nth(char_index).map_or(text.len(), |(index, _)| index)
}

/// ``/^\s+(?:`(?:\r\n|\r|\n)\s*)?/`` at `boundary` — a blank separator, or " ".
fn separator_after(resource: &str, boundary: usize) -> String {
    let byte = char_boundary(resource, boundary);
    let tail = &resource[byte..];
    let mut consumed = String::new();
    for c in tail.chars() {
        if c.is_whitespace() {
            consumed.push(c);
        } else {
            break;
        }
    }
    if consumed.is_empty() {
        return " ".into();
    }
    consumed
}

/// `Part` of a command word for directory extraction.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Word { text: String },
    CommandParameter { text: String },
}

/// `/^-[\p{L}\p{Nl}_?][\p{L}\p{Nl}\d_?-]*(?::|$)/` (prefix match).
fn is_parameter_shaped(raw: &str) -> bool {
    let mut characters = raw.chars();
    if characters.next() != Some('-') {
        return false;
    }
    let Some(first) = characters.next() else { return false };
    if !(first.is_alphanumeric() || first == '_' || first == '?') {
        return false;
    }
    let mut last: Option<char> = Some(first);
    for c in characters {
        if c.is_ascii_alphanumeric() || c == '_' || c == '?' || c == '-' {
            last = Some(c);
        } else {
            // The run stopped: a terminator must follow immediately.
            return c == ':';
        }
    }
    let _ = last;
    true
}

fn directory_parts(item: &ScannedCommand, words: &[String], powershell: bool) -> Vec<Part> {
    if !powershell {
        return item
            .raw_words
            .iter()
            .enumerate()
            .map(|(index, text)| {
                // Only literal quoting resolves to a static directory.
                let resolved = if text.starts_with("$'")
                    || (!text.contains(['$', '`', '~', '\\']) && text.contains(['\'', '"']))
                {
                    item.words.get(index).cloned().unwrap_or_else(|| text.clone())
                } else {
                    text.clone()
                };
                Part::Word { text: resolved }
            })
            .collect();
    }
    let mut parts = Vec::new();
    for (index, raw) in words.iter().enumerate() {
        let text = if !raw.contains("`$") && raw.contains(['\'', '"', '`']) {
            item.words.get(index).cloned().unwrap_or_else(|| raw.clone())
        } else {
            raw.replace("`$", "\0")
        };
        if !is_parameter_shaped(raw) {
            parts.push(Part::Word { text });
            continue;
        }
        match text.find(':') {
            Some(colon) if colon + 1 < text.len() => {
                parts.push(Part::CommandParameter { text: text[..colon].to_string() });
                parts.push(Part::Word { text: text[colon + 1..].to_string() });
            }
            Some(colon) => {
                parts.push(Part::CommandParameter { text: text[..colon].to_string() });
            }
            None => parts.push(Part::CommandParameter { text }),
        }
    }
    parts
}

/// The text payload of a [`Part`].
fn part_text(part: &Part) -> &str {
    match part {
        Part::Word { text } | Part::CommandParameter { text } => text,
    }
}

/// `POWERSHELL_PATH_FLAG_RE`.
fn is_powershell_path_flag(text: &str) -> bool {
    let lower = text.to_lowercase();
    let trimmed = lower.strip_suffix(':').unwrap_or(&lower);
    [
        "-p",
        "-pa",
        "-pat",
        "-path",
        "-lp",
        "-l",
        "-li",
        "-lit",
        "-lite",
        "-liter",
        "-litera",
        "-literal",
        "-literalp",
        "-literalpa",
        "-literalpat",
        "-literalpath",
        "-psp",
        "-pspa",
        "-pspath",
    ]
    .contains(&trimmed)
}

/// `directoryArgs` — the non-flag operands of a cwd command.
fn directory_args(
    command: &[Part],
    powershell: bool,
    cwd: &str,
    shell: &str,
) -> Vec<Option<String>> {
    if !powershell {
        return command
            .iter()
            .skip(1)
            .filter(|part| match part {
                Part::Word { text } => !text.starts_with('-'),
                Part::CommandParameter { .. } => false,
            })
            .filter_map(|part| match part {
                Part::Word { text } => directory_argument(text, powershell, cwd, shell),
                Part::CommandParameter { .. } => None,
            })
            .map(Some)
            .collect();
    }

    let mut directories = Vec::new();
    let mut path = false;
    let mut end_of_parameters = false;
    for part in command.iter().skip(1) {
        let text = match part {
            Part::Word { text } | Part::CommandParameter { text } => text.as_str(),
        };
        if !end_of_parameters && text == "--" {
            end_of_parameters = true;
            continue;
        }
        if path {
            if let Some(value) = directory_argument(text, powershell, cwd, shell) {
                directories.push(Some(value));
            }
            path = false;
            continue;
        }
        if !end_of_parameters && matches!(part, Part::CommandParameter { .. }) {
            path = is_powershell_path_flag(text);
            continue;
        }
        if let Some(value) = directory_argument(text, powershell, cwd, shell) {
            directories.push(Some(value));
        }
    }
    directories
}

fn home_dir() -> std::path::PathBuf {
    std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("/"), std::path::PathBuf::from)
}

/// `directoryArgument` — strip quotes, expand deterministic variables.
fn directory_argument(value: &str, powershell: bool, cwd: &str, shell: &str) -> Option<String> {
    let quote = value.chars().next();
    let text = if (quote == Some('"') || quote == Some('\''))
        && value.chars().last() == quote
        && value.chars().count() > 1
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    };
    if !powershell {
        return expand_known_directory(&text);
    }
    // PowerShell `$env:NAME` and the automatic `$HOME`/`$PWD`/`$PSHOME`
    // variables expand without executing code.
    let mut expanded = String::new();
    let characters: Vec<char> = text.chars().collect();
    let mut index = 0;
    'outer: while index < characters.len() {
        let rest: String = characters[index..].iter().collect();
        if rest.starts_with("${env:") {
            if let Some(close) = rest.find('}') {
                let key = &rest["${env:".len()..close];
                expanded.push_str(&environment_value(key));
                index += close + 1;
                continue;
            }
        }
        if let Some(rest) = rest.strip_prefix("$env:") {
            let key: String =
                rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !key.is_empty() {
                expanded.push_str(&environment_value(&key));
                index += "$env:".len() + key.chars().count();
                continue;
            }
        }
        let upper: String = rest.to_uppercase();
        for variable in ["HOME", "PWD", "PSHOME"] {
            let braced = format!("${{{variable}}}");
            if upper.starts_with(&braced) {
                expanded.push_str(&automatic_variable(variable, cwd, shell));
                index += braced.chars().count();
                continue 'outer;
            }
            if upper.starts_with(variable) {
                let after = rest.get(variable.len()..).and_then(|tail| tail.chars().next());
                if after.is_none() || after == Some('/') || after == Some('\\') {
                    expanded.push_str(&automatic_variable(variable, cwd, shell));
                    index += variable.len();
                    continue 'outer;
                }
            }
        }
        expanded.push(characters[index]);
        index += 1;
    }
    expand_known_directory(&expanded)
}

fn automatic_variable(variable: &str, cwd: &str, shell: &str) -> String {
    match variable {
        "HOME" => home_dir().to_string_lossy().to_string(),
        "PWD" => cwd.to_string(),
        _ => Path::new(shell).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
    }
}

fn environment_value(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

/// `expandKnownDirectory` — `~` expansion; unknown expressions stay
/// unresolved (None).
fn expand_known_directory(value: &str) -> Option<String> {
    // Unknown shell expressions cannot be resolved safely during permission
    // analysis.
    if value.contains('$') || value.contains('`') || value.starts_with('(') {
        return None;
    }
    let resolved = value.replace('\0', "$");
    if resolved == "~" {
        return Some(home_dir().to_string_lossy().to_string());
    }
    #[cfg(windows)]
    let home_tilde = resolved.starts_with("~/") || resolved.starts_with("~\\");
    #[cfg(not(windows))]
    let home_tilde = resolved.starts_with("~/");
    if home_tilde {
        return Some(home_dir().join(&resolved[2..]).to_string_lossy().to_string());
    }
    Some(resolved)
}

// ---------------------------------------------------------------------------
// Legacy (tree-sitter) scanner
// ---------------------------------------------------------------------------

/// `ShellParse.scanLegacy` — the tree-sitter walk.
pub fn scan_legacy(command: &str, shell: &str, cwd: &Path) -> Result<ScanOutput, String> {
    let powershell = select::ps(shell);
    let language = tree_sitter::Language::from(if powershell {
        tree_sitter_powershell::LANGUAGE
    } else {
        tree_sitter_bash::LANGUAGE
    });
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language)
        .map_err(|error| format!("Failed to parse shell command: {error}"))?;
    let tree =
        parser.parse(command, None).ok_or_else(|| "Failed to parse shell command".to_string())?;

    let mut output = ScanOutput::default();
    let root = tree.root_node();
    for node in descendants_of_kind(&root, "command") {
        let parts = node_parts(&node, command, powershell);
        let tokens: Vec<String> = parts.iter().map(|part| part_text(part).to_string()).collect();
        if tokens.is_empty() {
            continue;
        }
        let name = if powershell { tokens[0].to_lowercase() } else { tokens[0].clone() };
        if is_cwd_command(&name) {
            output.directories.extend(
                directory_args(&parts, powershell, &cwd.to_string_lossy(), shell)
                    .into_iter()
                    .flatten(),
            );
            continue;
        }
        let resource = match node.parent() {
            Some(parent) if parent.kind() == "redirected_statement" => {
                parent.utf8_text(command.as_bytes()).unwrap_or_default().trim().to_string()
            }
            _ => node.utf8_text(command.as_bytes()).unwrap_or_default().trim().to_string(),
        };
        let selected_length = prefix(&tokens).min(tokens.len());
        output.commands.push(ScannedCommandPermission {
            resource,
            save: format!("{} *", tokens[..selected_length].join(" ")),
        });
    }
    Ok(output)
}

/// `node.descendantsOfType("command")`.
fn descendants_of_kind<'a>(root: &tree_sitter::Node<'a>, kind: &str) -> Vec<tree_sitter::Node<'a>> {
    let mut found = Vec::new();
    let mut cursor = root.walk();
    loop {
        if cursor.node().kind() == kind {
            found.push(cursor.node());
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return found;
            }
        }
    }
}

/// `parts(node)` — the name/word children of a command node.
fn node_parts(node: &tree_sitter::Node<'_>, source: &str, powershell: bool) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return parts;
    }
    loop {
        let child = cursor.node();
        let kind = child.kind();
        let keep = |kind: &str| {
            if powershell {
                [
                    "command_name",
                    "command_name_expr",
                    "word",
                    "string",
                    "raw_string",
                    "concatenation",
                ]
                .contains(&kind)
            } else {
                ["command_name", "word", "string", "raw_string", "concatenation"].contains(&kind)
            }
        };
        if powershell && kind == "command_elements" {
            let mut inner = child.walk();
            if inner.goto_first_child() {
                loop {
                    let item = inner.node();
                    if item.kind() != "command_argument_sep" && item.kind() != "redirection" {
                        parts.push(Part::Word {
                            text: item.utf8_text(source.as_bytes()).unwrap_or_default().to_string(),
                        });
                    }
                    if !inner.goto_next_sibling() {
                        break;
                    }
                }
            }
        } else if keep(kind) {
            parts.push(Part::Word {
                text: child.utf8_text(source.as_bytes()).unwrap_or_default().to_string(),
            });
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    parts
}
