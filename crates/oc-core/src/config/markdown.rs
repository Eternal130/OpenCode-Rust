//! gray-matter style frontmatter parsing — port of `config/markdown.ts`.
//!
//! YAML frontmatter is parsed with `serde_yaml` (order preserving); on failure
//! the content is sanitized (unquoted colons rewritten as block scalars) and
//! parsed once more, so configs from other coding agents keep working.

use crate::config::Json;

/// Parsed markdown: frontmatter data (object) + body.
#[derive(Debug, Clone, PartialEq)]
pub struct Markdown {
    pub data: Json,
    pub content: String,
}

/// `ConfigMarkdown.parse`: parse, retrying once through [`sanitize`].
pub fn parse(content: &str) -> Result<Markdown, ParseError> {
    try_parse(content).or_else(|_| try_parse(&sanitize(content)))
}

/// `ConfigMarkdown.parseOption`.
#[must_use]
pub fn parse_option(content: &str) -> Option<Markdown> {
    parse(content).ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError;

fn try_parse(content: &str) -> Result<Markdown, ParseError> {
    let Some((frontmatter, body)) = split_frontmatter(content) else {
        // No frontmatter block: everything is body.
        return Ok(Markdown { data: Json::Object(Vec::new()), content: content.to_string() });
    };
    let value: serde_yaml::Value = serde_yaml::from_str(&frontmatter).map_err(|_| ParseError)?;
    Ok(Markdown { data: yaml_to_json(&value), content: body })
}

/// Splits a leading `---\n…\n---` block, mirroring the extraction regex used
/// by `sanitize` (`^---\r?\n([\s\S]*?)\r?\n---`).
fn split_frontmatter(content: &str) -> Option<(String, String)> {
    let rest = content.strip_prefix("---\n").or_else(|| content.strip_prefix("---\r\n"))?;
    let mut lines = rest.split_inclusive('\n').peekable();
    let mut frontmatter = String::new();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" {
            let body: String = lines.collect();
            return Some((frontmatter, body));
        }
        frontmatter.push_str(line);
    }
    None
}

/// `ConfigMarkdown.sanitize`: values containing unquoted colons are rewritten
/// as YAML block scalars.
#[must_use]
pub fn sanitize(content: &str) -> String {
    let Some((frontmatter, _)) = split_frontmatter(content) else {
        return content.to_string();
    };
    let mut result: Vec<String> = Vec::new();
    for line in frontmatter.split(['\n']) {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() || line.starts_with([' ', '\t']) {
            result.push(line.to_string());
            continue;
        }
        let Some((key, value)) = split_entry(line) else {
            result.push(line.to_string());
            continue;
        };
        let value = value.trim();
        if value.is_empty()
            || value == ">"
            || value == "|"
            || value.starts_with('"')
            || value.starts_with('\'')
        {
            result.push(line.to_string());
            continue;
        }
        if !value.contains(':') {
            result.push(line.to_string());
            continue;
        }
        result.push(format!("{key}: |-"));
        result.push(format!("  {value}"));
    }
    let sanitized = result.join("\n");
    content.replacen(&frontmatter, &sanitized, 1)
}

/// `^([a-zA-Z_][a-zA-Z0-9_]*)\s*:\s*(.*)$`
fn split_entry(line: &str) -> Option<(&str, &str)> {
    let (key, rest) = line.split_once(':')?;
    let mut key_chars = key.chars();
    let first = key_chars.next()?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return None;
    }
    if !key_chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    Some((key, rest))
}

fn yaml_to_json(value: &serde_yaml::Value) -> Json {
    match value {
        serde_yaml::Value::Null => Json::Null,
        serde_yaml::Value::Bool(value) => Json::Bool(*value),
        serde_yaml::Value::Number(value) => {
            if let Some(number) = value.as_i64().map(serde_json::Number::from) {
                Json::Number(number)
            } else if let Some(number) = value.as_u64().map(serde_json::Number::from) {
                Json::Number(number)
            } else {
                Json::Number(
                    value
                        .as_f64()
                        .and_then(serde_json::Number::from_f64)
                        .map_or(serde_json::Number::from(0), serde_json::Number::from),
                )
            }
        }
        serde_yaml::Value::String(value) => Json::String(value.clone()),
        serde_yaml::Value::Sequence(items) => Json::Array(items.iter().map(yaml_to_json).collect()),
        serde_yaml::Value::Mapping(mapping) => Json::Object(
            mapping
                .iter()
                .map(|(key, value)| {
                    (key.as_str().unwrap_or_default().to_string(), yaml_to_json(value))
                })
                .collect(),
        ),
        serde_yaml::Value::Tagged(tagged) => yaml_to_json(&tagged.value),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)] // deliberate empty-state assertions
    use super::*;

    #[test]
    fn config_markdown_parses_frontmatter_and_body() {
        let markdown =
            parse("---\nmodel: example/chat\ndescription: Review\n---\nReview carefully.\n")
                .unwrap();
        assert_eq!(markdown.data.get("model").and_then(Json::as_str), Some("example/chat"));
        assert_eq!(markdown.content, "Review carefully.\n");
    }

    #[test]
    fn config_markdown_without_frontmatter_is_all_body() {
        let markdown = parse("Just a body.").unwrap();
        assert!(markdown.data.as_object().unwrap().is_empty());
        assert_eq!(markdown.content, "Just a body.");
    }

    #[test]
    fn config_markdown_sanitizes_unquoted_colons() {
        // `description: Review: carefully` is invalid YAML (mapping in value).
        let raw = "---\ndescription: Review: carefully\n---\nBody";
        let markdown = parse(raw).unwrap();
        assert_eq!(
            markdown.data.get("description").and_then(Json::as_str),
            Some("Review: carefully")
        );
    }

    #[test]
    fn config_markdown_keeps_clean_lines_in_sanitize() {
        let raw = "---\n# comment\nmodel: example/chat\n  indented: value\nempty:\nblock: |\n  text\n---\nBody";
        assert_eq!(sanitize(raw), raw);
    }

    #[test]
    fn config_markdown_parse_option_returns_none_on_failure() {
        // A sequence item mixing a scalar and a mapping is invalid YAML that
        // sanitize cannot repair (no key:value line to rewrite).
        assert!(parse_option("---\n- just\n  a: list\n---\n").is_none());
    }

    #[test]
    fn config_markdown_preserves_key_order() {
        let markdown = parse("---\nzeta: 1\nalpha: 2\nmid: 3\n---\nB").unwrap();
        let keys: Vec<&str> =
            markdown.data.as_object().unwrap().iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, vec!["zeta", "alpha", "mid"]);
    }

    #[test]
    fn config_markdown_types_survive_conversion() {
        let markdown = parse("---\nbool: true\nint: 3\nfloat: 0.5\nlist:\n  - a\n  - b\nnested:\n  key: value\n---\nB").unwrap();
        assert_eq!(markdown.data.get("bool").and_then(Json::as_bool), Some(true));
        assert_eq!(
            markdown.data.get("float").and_then(|v| match v {
                Json::Number(n) => n.as_f64(),
                _ => None,
            }),
            Some(0.5)
        );
        assert_eq!(markdown.data.get("list").and_then(Json::as_array).map(<[Json]>::len), Some(2));
        assert_eq!(
            markdown.data.get("nested").and_then(|n| n.get("key")).and_then(Json::as_str),
            Some("value")
        );
    }
}
