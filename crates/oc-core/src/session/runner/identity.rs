//! Port of `plugin/identity.ts` — the "# Your Model" system-prompt
//! section injected from the active model's info (G-193.2).
//!
//! The ref plugin splices the section at index 1 of the system parts
//! (`event.system.splice(1, 0, ...)`) on every context/compaction/
//! generate request; the name comes from the catalog entry, falling
//! back to `Model.Info.default` (`name: id`).

/// `identity()` (identity.ts:9-16): the four lines joined by `\n`,
/// verbatim shape.
#[must_use]
pub fn identity_section(name: &str, provider_id: &str, model_id: &str) -> String {
    [
        "# Your Model".to_string(),
        format!("- Name: {name}"),
        format!("- Provider ID: {provider_id}"),
        format!("- Model ID: {model_id}"),
    ]
    .join("\n")
}

/// The section for a resolved reference — catalog `name` when the info
/// snapshot is present, else the `Model.Info.default` fallback
/// (`name: id`).
#[must_use]
pub fn identity_for(
    info: Option<&oc_schema::model::Info>,
    reference: &oc_schema::model::Ref,
) -> String {
    let name = info.map(|info| info.name.clone()).unwrap_or_else(|| reference.id.as_str().to_string());
    identity_section(&name, reference.provider_id.as_str(), reference.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_section_matches_ref_shape() {
        assert_eq!(
            identity_section("GPT-5", "openai", "gpt-5"),
            "# Your Model\n- Name: GPT-5\n- Provider ID: openai\n- Model ID: gpt-5"
        );
    }

    #[test]
    fn identity_falls_back_to_default_name() {
        let reference = oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("anthropic"),
            id: oc_schema::id::ModelId::parse("claude-sonnet-4"),
            variant: None,
        };
        let section = identity_for(None, &reference);
        assert!(section.contains("- Name: claude-sonnet-4"));
        assert!(section.contains("- Provider ID: anthropic"));
    }
}
