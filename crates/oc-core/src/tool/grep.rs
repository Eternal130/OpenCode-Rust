//! Port of `tool/plugin/grep.ts` + the `Ripgrep` runner — the content
//! search leaf spawning `rg --json` with the exact v2 argument construction
//! (core-runner-map §4.7). Also hosts the shared runner both search leaves
//! use (binary discovery: `OPENCODE_RG_PATH` → bundled `<exe_dir>/rg` →
//! `PATH`).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use oc_schema::tool::Content;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};

use super::{BuiltinTool, Context, ToolFailure, ToolOutcome, ToolServices};
use crate::file_access::{Invocation, ResolveInput};

/// `DEFAULT_SEARCH_LIMIT`.
pub const DEFAULT_SEARCH_LIMIT: usize = 100;
/// `DEFAULT_SEARCH_TIMEOUT_MS`.
pub const DEFAULT_SEARCH_TIMEOUT_MS: u64 = 30_000;
/// `MAX_SUBMATCHES`.
const MAX_SUBMATCHES: usize = 100;
/// Match-text clamp (Ripgrep.ts).
const MATCH_TEXT_CLAMP: usize = 2_000;

#[must_use]
pub fn timeout_message() -> String {
    format!(
        "Search timed out after {} seconds. Consider using a more specific path or pattern.",
        DEFAULT_SEARCH_TIMEOUT_MS / 1_000
    )
}

/// `normalizePath` — strip leading `./`/separators, forward slashes.
#[must_use]
pub fn normalize_path(value: &str) -> String {
    let mut out = value;
    loop {
        let stripped = out
            .strip_prefix("./")
            .or_else(|| out.strip_prefix(".\\"))
            .or_else(|| out.strip_prefix('/'))
            .or_else(|| out.strip_prefix('\\'));
        match stripped {
            Some(next) => out = next,
            None => break,
        }
    }
    out.replace('\\', "/")
}

/// One grep match row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub path: String,
    pub line: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RipgrepError {
    /// `InvalidPatternError` (exit 2 + regex parse error).
    InvalidPattern(String),
    /// Anything else (exit codes beyond 0/1/2, spawn failure).
    Failed(String),
}

impl std::fmt::Display for RipgrepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RipgrepError::InvalidPattern(stderr) => write!(f, "Invalid pattern: {stderr}"),
            RipgrepError::Failed(message) => write!(f, "{message}"),
        }
    }
}

/// Spawns `rg`, streams stdout lines, stops after `limit + 1` parsed rows.
/// `parse` maps a raw stdout line to a row; non-matching lines yield `None`.
async fn run_ripgrep<R>(
    cwd: &Path,
    args: &[String],
    limit: usize,
    parse: impl Fn(&str) -> Option<R> + Send,
) -> Result<Vec<R>, RipgrepError> {
    use tokio::process::Command;

    let binary = super::ripgrep_runner::binary();
    let mut child = Command::new(&binary)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| RipgrepError::Failed(format!("ripgrep execution failed: {error}")))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");

    let mut rows: Vec<R> = Vec::new();
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();
    let collect = async {
        while let Ok(Some(line)) = lines.next_line().await {
            if line.is_empty() {
                continue;
            }
            if let Some(row) = parse(&line) {
                rows.push(row);
                if rows.len() > limit {
                    break;
                }
            }
        }
    };
    let stderr_task = tokio::spawn(async move {
        let mut buffer = String::new();
        let mut reader = BufReader::new(stderr);
        let mut chunk = String::new();
        while let Ok(n) = reader.read_line(&mut chunk).await {
            if n == 0 {
                break;
            }
            if buffer.len() < 8 * 1024 {
                buffer.push_str(&chunk);
            }
            chunk.clear();
        }
        buffer
    });
    let timed_out = tokio::time::timeout(Duration::from_millis(DEFAULT_SEARCH_TIMEOUT_MS), collect)
        .await
        .is_err();
    if timed_out {
        let _ = child.kill().await;
        return Err(RipgrepError::Failed(timeout_message()));
    }
    let _ = child.kill().await;
    let stderr = stderr_task.await.unwrap_or_default();

    if rows.len() > limit {
        rows.truncate(limit);
        return Ok(rows);
    }
    let status = child.wait().await;
    let code = status.ok().and_then(|status| status.code());
    match code {
        Some(0) | Some(1) | None => Ok(rows),
        Some(2) => {
            if stderr.contains("regex parse error") || stderr.contains("error parsing regex") {
                Err(RipgrepError::InvalidPattern(stderr.trim().to_string()))
            } else {
                Err(RipgrepError::Failed(stderr.trim().to_string()))
            }
        }
        Some(code) => Err(RipgrepError::Failed(format!("ripgrep failed with code {code}"))),
    }
}

/// `Ripgrep.glob` — the exact v2 argument construction.
pub async fn ripgrep_glob(
    cwd: &Path,
    pattern: &str,
    hidden: bool,
    limit: usize,
) -> Result<Vec<String>, RipgrepError> {
    let mut args: Vec<String> = vec!["--no-config".into(), "--files".into()];
    if hidden {
        args.push("--hidden".into());
    }
    args.push(format!("--glob={pattern}"));
    if !hidden {
        // Positive globs override rg's hidden-file filter; exclude before
        // applying the result limit.
        args.push("--glob=!**/.*".into());
    }
    args.push("--glob=!**/.git/**".into());
    args.push(".".into());
    run_ripgrep(cwd, &args, limit, |line| Some(normalize_path(line))).await
}

/// `Ripgrep.grep` — the exact v2 argument construction.
pub async fn ripgrep_grep(
    cwd: &Path,
    pattern: &str,
    file: Option<&str>,
    include: Option<&str>,
    literal: bool,
    case_sensitive: bool,
    limit: usize,
) -> Result<Vec<SearchMatch>, RipgrepError> {
    let mut args: Vec<String> =
        vec!["--no-config".into(), "--json".into(), "--hidden".into(), "--no-messages".into()];
    if literal {
        args.push("--fixed-strings".into());
    }
    if !case_sensitive {
        args.push("--ignore-case".into());
    }
    if let Some(include) = include {
        args.push(format!("--glob={include}"));
    }
    args.push("--glob=!**/.git/**".into());
    args.push("--".into());
    args.push(pattern.to_string());
    args.push(file.unwrap_or(".").to_string());
    run_ripgrep(cwd, &args, limit, |line| {
        let Ok(json) = serde_json::from_str::<serde_json::Value>(line) else {
            return None;
        };
        if json["type"] != "match" {
            return None;
        }
        let data = &json["data"];
        let path = data["path"]["text"].as_str()?;
        let text = data["lines"]["text"].as_str()?;
        let line_number = data["line_number"].as_u64()?;
        let _ =
            data["submatches"].as_array().map(|submatches| submatches.len().min(MAX_SUBMATCHES));
        let text = if text.chars().count() > MATCH_TEXT_CLAMP {
            let prefix: String = text.chars().take(MATCH_TEXT_CLAMP).collect();
            format!("{prefix}...")
        } else {
            text.to_string()
        };
        Some(SearchMatch { path: normalize_path(path), line: line_number, text })
    })
    .await
}

pub fn description() -> String {
    "Search file contents using ripgrep's regular expression syntax or literal text matching. Use it to locate specific code, symbols, or text patterns, and narrow searches with `path` or `include`. Returns matching file paths, line numbers, and line previews.".to_string()
}

pub fn schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "minLength": 1,
                "x-opencode-message": "Pattern must not be empty",
                "description": "Regular expression or literal text to match in file contents."
            },
            "path": { "type": "string", "description": "File or directory to search. Defaults to the current working directory." },
            "include": { "type": "string", "description": "Glob pattern to filter files (for example, \"*.js\" or \"*.{ts,tsx}\")" },
            "literal": { "type": "boolean", "description": "Treat `pattern` as exact text instead of a regular expression (default: false)." },
            "caseSensitive": { "type": "boolean", "description": "Use case-sensitive matching (default: true)." },
            "limit": { "type": "integer", "minimum": 1, "description": "Maximum number of matching lines to return (default: 100)" }
        },
        "required": ["pattern"]
    })
}

pub fn tool(_services: &std::sync::Arc<ToolServices>) -> BuiltinTool {
    BuiltinTool {
        name: "grep",
        description: description(),
        input: schema(),
        permission: None,
        codemode: false,
        execute: std::sync::Arc::new(GrepExecutor),
    }
}

struct GrepExecutor;

#[async_trait::async_trait]
impl super::ToolExecuteWithContext for GrepExecutor {
    fn execute<'a>(
        &'a self,
        services: &'a ToolServices,
        input: &'a Value,
        context: &'a Context,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolOutcome, ToolFailure>> + Send + 'a>,
    > {
        Box::pin(async move { run(services, input, context).await })
    }
}

/// `toModelContent` (grep.ts).
#[must_use]
pub fn to_model_content(
    matches: &[SearchMatch],
    display_path_of: impl Fn(&str) -> PathBuf,
    truncated: bool,
) -> String {
    let mut lines: Vec<String> = if matches.is_empty() {
        vec!["No matches found".to_string()]
    } else {
        vec![format!("Found {} matches", matches.len())]
    };
    let mut current = String::new();
    for match_row in matches {
        let display = display_path_of(&match_row.path).to_string_lossy().replace('\\', "/");
        if current != display {
            if !current.is_empty() {
                lines.push(String::new());
            }
            current.clone_from(&display);
            lines.push(format!("{display}:"));
        }
        lines.push(format!("  Line {}: {}", match_row.line, match_row.text));
    }
    if truncated {
        lines.push(String::new());
        lines.push(format!(
            "(Results are truncated: showing first {} results. Consider using a more specific path or pattern.)",
            matches.len()
        ));
    }
    lines.join("\n")
}

async fn run(
    services: &ToolServices,
    input: &Value,
    context: &Context,
) -> Result<ToolOutcome, ToolFailure> {
    let pattern = input["pattern"].as_str().unwrap_or_default().to_string();
    let path = input["path"].as_str().map(str::to_string);
    let include = input["include"].as_str().map(str::to_string);
    let literal = input["literal"].as_bool().unwrap_or(false);
    let case_sensitive = input["caseSensitive"].as_bool().unwrap_or(true);
    let limit = input["limit"].as_u64().map_or(DEFAULT_SEARCH_LIMIT, |v| v as usize);

    let invocation = Invocation {
        session_id: context.session_id.clone(),
        agent: Some(context.agent.clone()),
        message_id: context.message_id.clone(),
        id: context.call_id.clone(),
    };
    let target = services
        .access
        .resolve(&ResolveInput { path: path.clone().unwrap_or_else(|| ".".into()), kind: None });
    services
        .access
        .authorize_external(std::slice::from_ref(&target), &invocation, None)
        .await
        .map_err(|error| blanket(&pattern, error))?;

    let mut assert_input = crate::permission::AssertInput::new(
        context.session_id.clone(),
        "grep",
        vec![pattern.clone()],
    );
    assert_input.agent = Some(context.agent.clone());
    assert_input.save = Some(vec!["*".to_string()]);
    assert_input.metadata = Some(
        serde_json::from_value(serde_json::json!({
            "root": ".",
            "path": path,
            "include": include,
            "literal": literal,
            "caseSensitive": case_sensitive,
            "limit": input["limit"].as_u64(),
        }))
        .unwrap_or_default(),
    );
    assert_input.source = super::tool_source(context);
    if let Err(error) = services.permission.assert(assert_input).await {
        return Err(blanket(&pattern, error));
    }

    let root = target.absolute.clone();
    let metadata = std::fs::metadata(&root).map_err(|_| {
        ToolFailure::message(format!(
            "Search path does not exist: {}",
            path.clone().unwrap_or_else(|| ".".into())
        ))
    })?;
    let (cwd, file): (PathBuf, Option<String>) = if metadata.is_dir() {
        (root.clone(), None)
    } else {
        (
            root.parent().map(Path::to_path_buf).unwrap_or_else(|| root.clone()),
            root.file_name().map(|name| name.to_string_lossy().into_owned()),
        )
    };

    let matches = ripgrep_grep(
        &cwd,
        &pattern,
        file.as_deref(),
        include.as_deref(),
        literal,
        case_sensitive,
        limit + 1,
    )
    .await
    .map_err(|error| match error {
        RipgrepError::InvalidPattern(stderr) => {
            ToolFailure::message(format!("Invalid regex pattern: {stderr}"))
        }
        RipgrepError::Failed(message) => {
            if message == timeout_message() {
                ToolFailure::message(message)
            } else {
                ToolFailure {
                    message: format!("Unable to grep for {pattern}"),
                    cause: Some(super::FailureCause::Nested(message)),
                }
            }
        }
    })?;
    let truncated = matches.len() > limit;
    let matches: Vec<SearchMatch> = matches.into_iter().take(limit).collect();

    let cwd_display = cwd.clone();
    let text = to_model_content(
        &matches,
        move |relative| crate::file_access::lexical_resolve(&cwd_display, Path::new(relative)),
        truncated,
    );
    Ok(ToolOutcome {
        content: vec![Content::Text { text }],
        metadata: Some(
            serde_json::from_value(serde_json::json!({
                "matches": matches.len(),
                "truncated": truncated,
            }))
            .unwrap_or_default(),
        ),
        output: None,
    })
}

fn blanket(pattern: &str, error: crate::permission::PermissionError) -> ToolFailure {
    ToolFailure {
        message: format!("Unable to grep for {pattern}"),
        cause: Some(super::permission_cause(&error)),
    }
}
