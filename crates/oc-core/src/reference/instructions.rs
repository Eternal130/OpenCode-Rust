//! Port of `packages/core/src/reference/instructions.ts` (v2.0.24): the
//! `core/reference-guidance` instruction block — the available project
//! references (description present), sorted by name, rendered as
//! `<available_references>` entries with small add/remove deltas.

use serde_json::Value;

use super::Info;

/// `Summary` (instructions.ts:8-13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
}

/// The instruction summaries: description present, sorted by `name`
/// (instructions.ts:69-76). `localeCompare` approximated ASCII-first
/// (same approach as the skill-guidance source; non-ASCII falls back to
/// code points — documented divergence).
#[must_use]
pub fn summaries(infos: &[Info]) -> Vec<Summary> {
    let mut summaries: Vec<Summary> = infos
        .iter()
        .filter(|info| info.description.is_some())
        .map(|info| Summary {
            name: info.name.clone(),
            path: info.path.to_string_lossy().into_owned(),
            description: info.description.clone(),
        })
        .collect();
    summaries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| b.name.cmp(&a.name))
    });
    summaries
}

/// One summary as canonical JSON (the instruction codec's blob shape).
#[must_use]
pub fn summary_value(summaries: &[Summary]) -> Value {
    Value::Array(
        summaries
            .iter()
            .map(|summary| {
                serde_json::json!({
                    "name": summary.name,
                    "path": summary.path,
                    "description": summary.description,
                })
            })
            .collect(),
    )
}

fn entries(summaries: &[Summary]) -> Vec<String> {
    summaries
        .iter()
        .flat_map(|summary| {
            Vec::from_iter([
                "  <reference>".to_owned(),
                format!("    <name>{}</name>", summary.name),
                format!("    <path>{}</path>", summary.path),
            ])
            .into_iter()
            .chain(summary.description.iter().map(|description| {
                format!("    <description>{description}</description>")
            }))
            .chain(std::iter::once("  </reference>".to_owned()))
        })
        .collect()
}

/// `render` (instructions.ts:23-29).
#[must_use]
pub fn render(summaries: &[Summary]) -> String {
    Vec::from_iter([
        "Project references provide additional directories that can be accessed when relevant."
            .to_owned(),
        "<available_references>".to_owned(),
    ])
    .into_iter()
    .chain(entries(summaries))
    .chain(std::iter::once("</available_references>".to_owned()))
    .collect::<Vec<_>>()
    .join("\n")
}

/// `update` (instructions.ts:31-54): additions and removals render as
/// small deltas; anything else restates the full list.
#[must_use]
pub fn update(previous: &[Summary], current: &[Summary]) -> String {
    let previous_names: Vec<&str> = previous.iter().map(|summary| summary.name.as_str()).collect();
    let current_names: Vec<&str> = current.iter().map(|summary| summary.name.as_str()).collect();
    let added: Vec<&Summary> =
        current.iter().filter(|summary| !previous_names.contains(&summary.name.as_str())).collect();
    let removed: Vec<&Summary> =
        previous.iter().filter(|summary| !current_names.contains(&summary.name.as_str())).collect();
    let changed = current.iter().any(|after| {
        previous
            .iter()
            .find(|before| before.name == after.name)
            .is_some_and(|before| before.path != after.path || before.description != after.description)
    });

    if changed || (added.is_empty() && removed.is_empty()) {
        return [
            "The available project references have changed. This list supersedes the previous reference list."
                .to_owned(),
            render(current),
        ]
        .join("\n");
    }

    let mut parts = Vec::new();
    if !added.is_empty() {
        parts.push("New project references are available in addition to those previously listed:".to_owned());
        parts.extend(entries(&added.iter().map(|summary| (*summary).clone()).collect::<Vec<_>>()));
    }
    if !removed.is_empty() {
        parts.push(format!(
            "The following project references are no longer available and must not be used: {}.",
            removed.iter().map(|summary| summary.name.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    parts.join("\n")
}

/// The `removed` render (instructions.ts:84-86).
pub const REMOVED_TEXT: &str =
    "Project reference guidance is no longer available. Do not use previously listed references.";
