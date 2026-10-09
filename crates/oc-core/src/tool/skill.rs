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

/// `Skill.prepare` — the model-facing output for one skill: the front-matter
/// body plus referenced files (`instructions.md` convention: name directory,
/// `output` = content).
pub fn prepare(skill: &Skill) -> std::io::Result<(String, String)> {
    // name + directory from the SKILL.md path; output = the skill content
    // joined with the referenced files inside the directory.
    let path = std::path::Path::new(skill.path.as_str());
    let directory =
        path.parent().map(std::path::Path::to_string_lossy).unwrap_or_default().to_string();
    let mut output = skill.content.clone();
    if let Ok(entries) = std::fs::read_dir(&directory) {
        let mut references: Vec<String> = entries
            .flatten()
            .filter(|entry| {
                entry.path().is_file() && entry.file_name().to_string_lossy() != "SKILL.md"
            })
            .map(|entry| entry.path().to_string_lossy().to_string())
            .collect();
        references.sort();
        for reference in references {
            if let Ok(text) = std::fs::read_to_string(&reference) {
                if !output.is_empty() {
                    output.push_str("\n\n");
                }
                output.push_str(&text);
            }
        }
    }
    Ok((skill.name.to_string(), output))
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
        std::fs::write(directory.path().join("reference.md"), "reference").unwrap();
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
        assert_eq!(output, "# Effect\n\nGuidance\n\nreference");
    }

    #[test]
    fn tool_skill_missing_id_fails() {
        let table = MemorySkillTable::default();
        assert!(table.get("nope").is_none());
    }
}
