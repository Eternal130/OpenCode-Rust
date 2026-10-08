//! `{env:VAR}` / `{file:path}` substitution — port of `config/variable.ts`.

use std::path::{Path, PathBuf};

use std::sync::LazyLock;

use regex::Regex;

use crate::config::resolve_path;

/// Substitution failure (a bad `{file:}` reference).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubstituteError {
    /// Config file path or virtual source the text came from.
    pub source: String,
    pub message: String,
}

/// What a missing `{file:}` target does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Missing {
    /// Fail with `bad file reference` (config documents are skipped).
    #[default]
    Error,
    /// Substitute an empty string.
    Empty,
}

/// Where the text came from; determines the base directory for relative
/// `{file:}` references.
#[derive(Debug, Clone)]
pub enum Source {
    /// A file on disk; references resolve against its directory.
    Path { path: PathBuf },
    /// Virtual content (inline/env documents) with an explicit base dir.
    Virtual { source: String, dir: PathBuf },
}

impl Source {
    fn dir(&self) -> PathBuf {
        match self {
            Source::Path { path } => path.parent().map(Path::to_path_buf).unwrap_or_default(),
            Source::Virtual { dir, .. } => dir.clone(),
        }
    }

    fn name(&self) -> String {
        match self {
            Source::Path { path } => path.display().to_string(),
            Source::Virtual { source, .. } => source.clone(),
        }
    }
}

/// Input to [`substitute`].
pub struct Input<'a> {
    pub source: Source,
    pub text: &'a str,
    /// Env lookup; wellknown documents layer credential overrides here.
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub missing: Missing,
}

static ENV_TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{env:([^}]+)\}").unwrap());
static FILE_TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{file:[^}]+\}").unwrap());

/// Applies `{env:VAR}` then `{file:path}` substitution. Missing env variables
/// substitute `""`; `{file:}` on `//`-comment lines is left untouched; file
/// content is trimmed and JSON-escaped inline.
pub fn substitute(input: &Input) -> Result<String, SubstituteError> {
    let text = substitute_env(input.text, input.env);
    if !text.contains("{file:") {
        return Ok(text);
    }
    substitute_files(input, &text)
}

fn substitute_env(text: &str, env: &dyn Fn(&str) -> Option<String>) -> String {
    ENV_TOKEN
        .replace_all(text, |captures: &regex::Captures| env(&captures[1]).unwrap_or_default())
        .into_owned()
}

fn substitute_files(input: &Input, text: &str) -> Result<String, SubstituteError> {
    let config_dir = input.source.dir();
    let config_source = input.source.name();
    let mut out = String::new();
    let mut cursor = 0usize;

    for captures in FILE_TOKEN.captures_iter(text) {
        let token = captures.get(0).expect("whole match");
        let index = token.start();
        out.push_str(&text[cursor..index]);

        // `{file:}` tokens on `//` comment lines are left verbatim.
        let line_start = text[..index].rfind('\n').map_or(0, |position| position + 1);
        let prefix = text[line_start..index].trim_start();
        if prefix.starts_with("//") {
            out.push_str(token.as_str());
            cursor = token.end();
            continue;
        }

        let file_path = &token.as_str()["{file:".len()..token.as_str().len() - 1];
        let expanded = expand_home(file_path);
        let resolved =
            if expanded.is_absolute() { expanded } else { clean_join(&config_dir, &expanded) };
        let content = match std::fs::read_to_string(&resolved) {
            Ok(content) => content,
            Err(_) if input.missing == Missing::Empty => String::new(),
            Err(error) => {
                let message = if error.kind() == std::io::ErrorKind::NotFound {
                    format!(
                        "bad file reference: \"{}\" {} does not exist",
                        token.as_str(),
                        resolved.display()
                    )
                } else {
                    format!("bad file reference: \"{}\"", token.as_str())
                };
                return Err(SubstituteError { source: config_source, message });
            }
        };

        out.push_str(&json_escape_inline(content.trim()));
        cursor = token.end();
    }

    out.push_str(&text[cursor..]);
    Ok(out)
}

/// `JSON.stringify(value).slice(1, -1)` — the escaped content without the
/// surrounding quotes.
fn json_escape_inline(value: &str) -> String {
    let quoted = serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string());
    quoted[1..quoted.len() - 1].to_string()
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return Path::new(&home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn clean_join(base: &Path, relative: &Path) -> PathBuf {
    let joined = base.join(relative);
    let mut out = PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolves `path` lexically against the process cwd (test helper mirroring
/// `path.resolve`).
#[must_use]
pub fn absolute(path: &Path) -> PathBuf {
    resolve_path(path)
}

#[cfg(test)]
mod tests {
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

    fn none_env(_: &str) -> Option<String> {
        None
    }

    fn lookup<'a>(names: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| names.iter().find(|(key, _)| *key == name).map(|(_, value)| value.to_string())
    }

    #[test]
    fn config_variable_env_substitutes_and_missing_becomes_empty() {
        let env = lookup(&[("SET", "value")]);
        let text = substitute_env("a-{env:SET}-b-{env:UNSET}-c", &env);
        assert_eq!(text, "a-value-b--c");
    }

    #[test]
    fn config_variable_file_relative_to_config_dir() {
        let dir = tempdir("variable-rel");
        fs::write(dir.join("token.txt"), "  secret value \n").unwrap();
        let input = Input {
            source: Source::Path { path: dir.join("opencode.jsonc") },
            text: r#""X-Token": "{file:token.txt}""#,
            env: &none_env,
            missing: Missing::Error,
        };
        assert_eq!(substitute(&input).unwrap(), r#""X-Token": "secret value""#);
    }

    #[test]
    fn config_variable_file_json_escapes_inline() {
        let dir = tempdir("variable-escape");
        fs::write(dir.join("token.txt"), "file\n\"token\"\n").unwrap();
        let input = Input {
            source: Source::Path { path: dir.join("opencode.json") },
            text: r#""X": "{file:token.txt}""#,
            env: &none_env,
            missing: Missing::Error,
        };
        assert_eq!(substitute(&input).unwrap(), r#""X": "file\n\"token\"""#);
    }

    #[test]
    fn config_variable_comment_line_tokens_untouched() {
        let input = Input {
            source: Source::Path { path: PathBuf::from("/config/opencode.jsonc") },
            text: "// ignored: {file:missing.txt}\n\"a\": 1",
            env: &none_env,
            missing: Missing::Error,
        };
        assert_eq!(substitute(&input).unwrap(), "// ignored: {file:missing.txt}\n\"a\": 1");
    }

    #[test]
    fn config_variable_missing_file_errors_with_source() {
        let dir = tempdir("variable-missing");
        let input = Input {
            source: Source::Path { path: dir.join("opencode.json") },
            text: r"{file:gone.txt}",
            env: &none_env,
            missing: Missing::Error,
        };
        let error = substitute(&input).unwrap_err();
        assert_eq!(error.source, dir.join("opencode.json").display().to_string());
        assert!(error.message.contains("bad file reference"));
        assert!(error.message.contains("gone.txt"));
    }

    #[test]
    fn config_variable_missing_empty_mode_substitutes_nothing() {
        let dir = tempdir("variable-empty");
        let input = Input {
            source: Source::Path { path: dir.join("opencode.json") },
            text: r#""X": "{file:gone.txt}""#,
            env: &none_env,
            missing: Missing::Empty,
        };
        assert_eq!(substitute(&input).unwrap(), r#""X": """#);
    }

    #[test]
    fn config_variable_absolute_references_ignore_the_base_dir() {
        let dir = tempdir("variable-virtual");
        let absolute = dir.join("base.txt");
        fs::write(&absolute, "from-absolute").unwrap();
        let text = format!(r#""Y": "{{file:{}}}""#, absolute.display());
        let input = Input {
            source: Source::Virtual {
                source: "OPENCODE_CONFIG_CONTENT".into(),
                dir: dir.join("nested"),
            },
            text: &text,
            env: &none_env,
            missing: Missing::Error,
        };
        assert_eq!(substitute(&input).unwrap(), r#""Y": "from-absolute""#);
    }
}
