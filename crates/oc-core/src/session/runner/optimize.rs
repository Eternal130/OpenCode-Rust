//! Port of `plugin/optimize.ts` — the per-provider system-prompt
//! optimization templates (G-193.4).
//!
//! The ref plugin table (`Plugins`, optimize.ts:62) applies the FIRST
//! matching family template to `event.system[0]` — `override` mode
//! replaces the text with the rendered template, `append` (anthropic)
//! concatenates. Skipped entirely when the active agent carries a
//! custom system prompt (optimize.ts:81). Rendering goes through the
//! same `SessionSystemPrompt.render` as the base template (the
//! `${OPENCODE_TOOL_GUIDANCE}` substitution).
//!
//! The `OpenAIToolsPlugin`/`AnthropicToolsPlugin` tool-curation halves
//! are disabled in the ref (optimize.ts:32-33) and are not ported.

/// `plugin/system-prompt/gpt.txt`, verbatim.
pub const PROMPT_GPT: &str = include_str!("optimize/gpt.txt");
/// `plugin/system-prompt/gpt-astra.txt`, verbatim.
pub const PROMPT_ASTRA: &str = include_str!("optimize/gpt-astra.txt");
/// `plugin/system-prompt/kimi.txt`, verbatim.
pub const PROMPT_KIMI: &str = include_str!("optimize/kimi.txt");
/// `plugin/system-prompt/meta.txt`, verbatim ({{MODEL_NAME}} intact).
pub const PROMPT_META: &str = include_str!("optimize/meta.txt");
/// `plugin/system-prompt/trinity.txt`, verbatim.
pub const PROMPT_TRINITY: &str = include_str!("optimize/trinity.txt");
/// `plugin/system-prompt/anthropic.txt`, verbatim.
pub const PROMPT_ANTHROPIC: &str = include_str!("optimize/anthropic.txt");

/// One table entry: family predicate over the catalog model, the raw
/// template, and the apply mode.
struct Family {
    matches: fn(&oc_schema::model::Info) -> bool,
    template: fn(&oc_schema::model::Info) -> String,
    append: bool,
}

/// The plugin table in `Plugins` order (optimize.ts:16-62).
fn families() -> Vec<Family> {
    fn lower_id(model: &oc_schema::model::Info) -> String {
        model.id.as_str().to_lowercase()
    }
    vec![
        // opencode.prompt.openai — gpt family, astra splits to its own
        // template (optimize.ts:16-20).
        Family {
            matches: |model| lower_id(model).contains("gpt"),
            template: |model| {
                if lower_id(model).contains("astra") {
                    PROMPT_ASTRA.to_string()
                } else {
                    PROMPT_GPT.to_string()
                }
            },
            append: false,
        },
        // opencode.prompt.anthropic — claude family, APPEND mode
        // (optimize.ts:22-30).
        Family {
            matches: |model| lower_id(model).contains("claude"),
            template: |_| PROMPT_ANTHROPIC.to_string(),
            append: true,
        },
        // opencode.prompt.kimi (optimize.ts:51-53).
        Family {
            matches: |model| lower_id(model).contains("kimi"),
            template: |_| PROMPT_KIMI.to_string(),
            append: false,
        },
        // opencode.prompt.arcee — trinity (optimize.ts:54-56).
        Family {
            matches: |model| lower_id(model).contains("trinity"),
            template: |_| PROMPT_TRINITY.to_string(),
            append: false,
        },
        // opencode.prompt.meta — muse, {{MODEL_NAME}} substituted
        // (optimize.ts:57-60).
        Family {
            matches: |model| lower_id(model).contains("muse"),
            template: |model| PROMPT_META.replace("{{MODEL_NAME}}", &model.name),
            append: false,
        },
    ]
}

/// `optimize(model, tools)` + the system[0] rewrite (optimize.ts:79-86):
/// the first matching family template rendered through the shared
/// system-prompt renderer; `override` replaces the base text, `append`
/// concatenates (`${system}\n\n${rendered}`). No match → the base text
/// unchanged.
#[must_use]
pub fn apply(base: &str, model: Option<&oc_schema::model::Info>, tools: &[String]) -> String {
    let Some(model) = model else { return base.to_string() };
    for family in families() {
        if !(family.matches)(model) {
            continue;
        }
        let rendered = super::system_prompt::render(&(family.template)(model), tools);
        return if family.append { format!("{base}\n\n{rendered}") } else { rendered };
    }
    base.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, name: &str) -> oc_schema::model::Info {
        let mut info = oc_schema::model::Info::default_for(
            oc_schema::id::ProviderId::parse("test"),
            oc_schema::id::ModelId::parse(id),
        );
        info.name = name.to_string();
        info
    }

    #[test]
    fn gpt_family_overrides_with_gpt_template() {
        let base = super::super::system_prompt::make(&["shell".to_string()]);
        let out = apply(&base, Some(&model("gpt-5.1", "GPT-5.1")), &["shell".to_string()]);
        assert!(out.starts_with("You are an AI agent running in OpenCode"));
        assert!(out.contains("## Intermediate Commentary"));
        assert!(out.contains("- Prefer dedicated tools over shell commands"));
    }

    #[test]
    fn astra_id_selects_the_astra_template() {
        let out = apply("base", Some(&model("gpt-5-astra", "Astra")), &[]);
        assert!(out.contains("Do not use a skill based solely on keywords"));
        assert!(!out.contains("## Intermediate Commentary\n\nAs you work, you send messages to the commentary channel. As you work"));
    }

    #[test]
    fn claude_family_appends_the_anthropic_section() {
        let base = "BASE";
        let out = apply(base, Some(&model("claude-sonnet-4", "Claude")), &[]);
        assert!(out.starts_with("BASE\n\n# Code comments"));
        assert!(out.contains("match the surrounding comment density"));
    }

    #[test]
    fn kimi_trinity_meta_families_each_match_their_template() {
        let kimi = apply("b", Some(&model("kimi-k2", "Kimi")), &[]);
        assert!(kimi.starts_with("You are OpenCode, an interactive general AI agent"));
        let trinity = apply("b", Some(&model("trinity-1", "Trinity")), &[]);
        assert!(trinity.starts_with("You are opencode, an interactive CLI tool"));
        let meta = apply("b", Some(&model("muse-1", "Muse One")), &[]);
        assert!(meta
            .contains("You are powered by Muse One, a large language model trained by Meta MSL."));
        assert!(!meta.contains("{{MODEL_NAME}}"));
    }

    #[test]
    fn non_matching_family_leaves_the_base_text() {
        assert_eq!(apply("BASE", Some(&model("gemini-3", "Gemini")), &[]), "BASE");
        assert_eq!(apply("BASE", None, &[]), "BASE");
    }
}
