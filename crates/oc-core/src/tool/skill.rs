//! Port of `packages/core/src/tool/plugin/skill.ts` — load a skill's
//! instructions (§4.12). Skill discovery belongs to the config lane; this
//! leaf resolves against a [`SkillTable`] seam.

use std::sync::Arc;

use oc_schema::skill::Skill;

use crate::permission::{AssertInput, PermissionError};
use crate::tool::{
    permission_cause, BuiltinTool, ToolExecuteWithContext, ToolFailure, ToolOutcome, ToolServices,
};

pub fn description() -> String {
    "Load a specialized skill's instructions and resources into the current conversation when the task at hand matches its description.\n\nThe skill ID must match an available skill or a skill explicitly referenced by the user.".to_string()
}

/// `Skill.prepare` (ref skill.ts:53-62 + plugin/skill.ts:38): the model
/// facing output is the `<skill_content>` ENVELOPE (trimmed body, base
/// directory note, and a sampled file-path listing — absolute, sorted,
/// capped at 10, `SKILL.md` excluded) — never the raw file contents.
pub fn prepare(skill: &Skill) -> std::io::Result<(String, String)> {
    let files = sample_skill_files(skill)?;
    Ok((skill.name.to_string(), to_model_output(skill, files)))
}

/// `Skill.prepare(...).output` for the PROMPT path (core/session/prompt.ts:71
/// → `Skill.toModelOutput`): the same envelope injected for an `@skill`
/// attachment.
pub fn prompt_text(skill: &Skill) -> std::io::Result<String> {
    let files = sample_skill_files(skill)?;
    Ok(to_model_output(skill, files))
}

/// `toModelOutput` (ref skill.ts:33-50) — the `<skill_content>` envelope.
fn to_model_output(skill: &Skill, files: Vec<String>) -> String {
    let path = std::path::Path::new(skill.path.as_str());
    let directory =
        path.parent().map(std::path::Path::to_string_lossy).unwrap_or_default().to_string();
    let mut output = String::new();
    output.push_str(&format!("<skill_content name=\"{}\">\n", skill.name.as_str()));
    output.push_str(&format!("# Skill: {}\n\n", skill.name.as_str()));
    output.push_str(skill.content.trim());
    output.push_str("\n\n");
    output.push_str(&format!("Base directory for this skill: {directory}\n"));
    output.push_str(
        "Relative paths in this skill (e.g., scripts/, reference/) are relative to this base directory.\n",
    );
    output.push_str("Note: file list is sampled.\n\n");
    output.push_str("<skill_files>\n");
    for file in &files {
        output.push_str(&format!("<file>{file}</file>\n"));
    }
    output.push_str("</skill_files>\n");
    output.push_str("</skill_content>");
    output
}

/// `fs.scan("**/*", { include: "file", dot: true })` under the skill
/// directory — only `SKILL.md` skills sample files (skill.ts:58-62);
/// a bare skill file contributes an empty list. Sorted, sliced to 10
/// (ref skill.ts:59-63).
fn sample_skill_files(skill: &Skill) -> std::io::Result<Vec<String>> {
    let path = std::path::Path::new(skill.path.as_str());
    let directory =
        path.parent().map(std::path::Path::to_string_lossy).unwrap_or_default().to_string();
    let mut files: Vec<String> = Vec::new();
    if path.file_name().is_some_and(|name| name == "SKILL.md") {
        collect_skill_files(std::path::Path::new(&directory), &mut files, 0);
        files.sort();
        files.truncate(10);
    }
    Ok(files)
}

/// Recursive file sampler (`fs.scan("**/*", { include: "file", dot: true })`,
/// `SKILL.md` basename excluded, bounded by the 10-file sample + depth cap).
fn collect_skill_files(directory: &std::path::Path, out: &mut Vec<String>, depth: usize) {
    const MAX_DEPTH: usize = 6;
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_skill_files(&path, out, depth + 1);
        } else if path.is_file() && path.file_name().is_some_and(|name| name != "SKILL.md") {
            out.push(path.to_string_lossy().into_owned());
        }
    }
}

/// The skill resolution seam (`Skill.Service.get`) — the config-lane table.
pub trait SkillTable: Send + Sync {
    fn get(&self, id: &str) -> Option<Skill>;
}

/// An in-memory table (tests + until the config lane exposes its service).
#[derive(Default)]
pub struct MemorySkillTable {
    pub skills: Vec<Skill>,
}

impl SkillTable for MemorySkillTable {
    fn get(&self, id: &str) -> Option<Skill> {
        self.skills.iter().find(|skill| skill.id.as_str() == id).cloned()
    }
}

/// The skill leaf executor.
pub struct SkillExecute {
    pub table: Arc<dyn SkillTable>,
}

#[derive(serde::Deserialize)]
struct Input {
    id: String,
}

impl ToolExecuteWithContext for SkillExecute {
    fn execute<'a>(
        &'a self,
        services: &'a ToolServices,
        input: &'a serde_json::Value,
        context: &'a crate::location::ToolCallContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolOutcome, ToolFailure>> + Send + 'a>,
    > {
        let input = input.clone();
        let context = context.clone();
        let table = Arc::clone(&self.table);
        Box::pin(async move {
            let input: Input = serde_json::from_value(input)
                .map_err(|error| ToolFailure::message(format!("Invalid arguments: {error}")))?;
            let Some(skill) = table.get(&input.id) else {
                return Err(ToolFailure {
                    message: format!("Unable to load skill {}", input.id),
                    cause: None,
                });
            };
            services
                .permission
                .assert(AssertInput {
                    id: None,
                    session_id: context.session_id.clone(),
                    agent: Some(context.agent.clone()),
                    action: "skill".into(),
                    resources: vec![skill.id.to_string()],
                    save: Some(vec![skill.id.to_string()]),
                    metadata: None,
                    source: Some(oc_schema::permission::Source::Tool {
                        message_id: context.message_id.clone(),
                        id: context.call_id.clone(),
                    }),
                })
                .await
                .map_err(|error: PermissionError| ToolFailure {
                    message: format!("Unable to load skill {}", input.id),
                    cause: Some(permission_cause(&error)),
                })?;
            let (name, output) = prepare(&skill).map_err(|error| ToolFailure {
                message: format!("Unable to load skill {}", input.id),
                cause: Some(super::FailureCause::Nested(error.to_string())),
            })?;
            let directory = std::path::Path::new(skill.path.as_str())
                .parent()
                .map(std::path::Path::to_string_lossy)
                .unwrap_or_default()
                .to_string();
            Ok(ToolOutcome {
                content: vec![oc_schema::tool::Content::Text { text: output.clone() }],
                metadata: Some(
                    serde_json::from_value(
                        serde_json::json!({ "name": name, "directory": directory }),
                    )
                    .unwrap_or_default(),
                ),
                output: Some(
                    serde_json::json!({ "name": name, "directory": directory, "output": output }),
                ),
            })
        })
    }
}

/// The skill builtin registration.
pub fn tool(table: Arc<dyn SkillTable>) -> BuiltinTool {
    BuiltinTool {
        name: "skill",
        description: description(),
        input: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "The ID of an available skill or a skill explicitly referenced by the user" }
            },
            "required": ["id"]
        }),
        permission: Some("skill"),
        codemode: false,
        namespace: None,
        execute: Arc::new(SkillExecute { table }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oc_schema::common::AbsolutePath;
    use oc_schema::id::{SkillId, SkillName};

    #[test]
    fn tool_skill_prepare_loads_model_facing_content() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("SKILL.md"), "unused").unwrap();
        std::fs::write(directory.path().join("reference.md"), "reference contents").unwrap();
        let skill = Skill {
            id: SkillId::parse("effect"),
            name: SkillName::parse("Effect"),
            description: Some("Use Effect".into()),
            auto_invoke: None,
            path: AbsolutePath::parse(directory.path().join("SKILL.md").to_str().unwrap()),
            content: "# Effect\n\nGuidance".into(),
        };
        let (name, output) = prepare(&skill).unwrap();
        assert_eq!(name, "Effect");
        // G-53: the envelope lists file PATHS, never contents.
        assert!(output.starts_with("<skill_content name=\"Effect\">\n# Skill: Effect\n"), "{output}");
        let listed_reference = format!("<file>{}</file>", directory.path().join("reference.md").display());
        assert!(output.contains(&listed_reference), "{output}");
        assert!(!output.contains("reference contents"), "no file contents in the output");
        assert!(output.ends_with("</skill_files>\n</skill_content>"));
        // The prompt path shares the same envelope.
        assert_eq!(prompt_text(&skill).unwrap(), output);
    }

    // G-53: >10 files ⇒ at most 10 sampled paths (ref skill.ts:63
    // `.slice(0, 10)`), and no file contents ride the output.
    #[test]
    fn tool_skill_prepare_caps_the_file_listing_at_ten() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("SKILL.md"), "unused").unwrap();
        for n in 0..15 {
            std::fs::write(directory.path().join(format!("f{n:02}.txt")), format!("contents {n}")).unwrap();
        }
        let skill = Skill {
            id: SkillId::parse("many"),
            name: SkillName::parse("Many"),
            description: None,
            auto_invoke: None,
            path: AbsolutePath::parse(directory.path().join("SKILL.md").to_str().unwrap()),
            content: "Guidance".into(),
        };
        let (_, output) = prepare(&skill).unwrap();
        let listed = output.matches("<file>").count();
        assert_eq!(listed, 10, "exactly the 10-file sample cap: {output}");
        for n in 0..15 {
            assert!(!output.contains(&format!("contents {n}")), "no file contents: {output}");
        }
    }

    #[test]
    fn tool_skill_missing_id_fails() {
        let table = MemorySkillTable::default();
        assert!(table.get("nope").is_none());
    }
}
