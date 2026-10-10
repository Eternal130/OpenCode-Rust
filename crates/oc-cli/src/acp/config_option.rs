//! Config-option catalog shaping (port of `acp/config-option.ts`): the
//! model/effort/mode `SessionConfigOption` list and selection changes.

use agent_client_protocol::schema::v1::{
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption,
};
use serde_json::Value;

use crate::acp::catalog::{Catalog, Model, ModelRef};
use crate::acp::error::AcpError;

const DEFAULT_VARIANT_VALUE: &str = "default";

/// `Selection` (config-option.ts:11-14).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    pub model: Option<ModelRef>,
    pub mode_id: Option<String>,
}

/// `Change` (config-option.ts:16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Model(ModelRef),
    Mode(String),
}

/// `currentModel` (config-option.ts:18-20).
#[must_use]
pub fn current_model(catalog: &Catalog, selection: &Selection) -> ModelRef {
    selection.model.clone().unwrap_or_else(|| catalog.default_model.clone())
}

/// `configOptions` (config-option.ts:22-68): model, effort (when the
/// model has variants), then mode.
#[must_use]
pub fn config_options(catalog: &Catalog, selection: &Selection) -> Vec<SessionConfigOption> {
    let model = current_model(catalog, selection);
    let variants =
        catalog.find_model(&model).map(|model| model.variants.clone()).unwrap_or_default();
    let mut options = Vec::new();

    let mut sorted_models = catalog.models.clone();
    sorted_models
        .sort_by(|a, b| a.provider_id.cmp(&b.provider_id).then_with(|| a.name.cmp(&b.name)));
    options.push(option(
        "model",
        "Model",
        "",
        SessionConfigOptionCategory::Model,
        format!("{}/{}", model.provider_id, model.id),
        sorted_models
            .into_iter()
            .map(|item| {
                choice(
                    format!("{}/{}", item.provider_id, item.id),
                    format!("{}/{}", item.provider_id, item.name),
                    None,
                )
            })
            .collect(),
    ));

    if !variants.is_empty() {
        let mut effort_values: Vec<String> = variants.clone();
        if !effort_values.iter().any(|value| value == DEFAULT_VARIANT_VALUE) {
            effort_values.push(DEFAULT_VARIANT_VALUE.to_owned());
        }
        options.push(option(
            "effort",
            "Effort",
            "Available effort levels for this model",
            SessionConfigOptionCategory::ThoughtLevel,
            select_variant(model.variant.as_deref(), &variants),
            effort_values
                .into_iter()
                .map(|variant| {
                    let name = variant
                        .split(['_', '-'])
                        .map(|part| {
                            let mut characters = part.chars();
                            match characters.next() {
                                Some(first) => {
                                    first.to_uppercase().collect::<String>() + characters.as_str()
                                }
                                None => String::new(),
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    choice(variant, name, None)
                })
                .collect(),
        ));
    }

    options.push(option(
        "mode",
        "Session Mode",
        "",
        SessionConfigOptionCategory::Mode,
        selection.mode_id.clone().unwrap_or_else(|| catalog.default_mode_id.clone()),
        catalog
            .modes
            .iter()
            .map(|mode| choice(mode.id.clone(), mode.name.clone(), mode.description.clone()))
            .collect(),
    ));
    options
}

fn option(
    id: &str,
    name: &str,
    description: &str,
    category: SessionConfigOptionCategory,
    current_value: String,
    choices: Vec<SessionConfigSelectOption>,
) -> SessionConfigOption {
    let configured =
        SessionConfigOption::select(id.to_owned(), name.to_owned(), current_value, choices)
            .category(category);
    if description.is_empty() {
        configured
    } else {
        configured.description(description)
    }
}

fn choice(value: String, name: String, description: Option<String>) -> SessionConfigSelectOption {
    let option = SessionConfigSelectOption::new(value, name);
    match description {
        Some(description) => option.description(description),
        None => option,
    }
}

/// `availableCommands` (config-option.ts:70-75).
#[must_use]
pub fn available_commands(catalog: &Catalog) -> Vec<Value> {
    let mut commands: Vec<Value> = catalog
        .commands
        .iter()
        .map(|command| {
            json_command(&command.name, command.description.as_deref().unwrap_or_default())
        })
        .collect();
    commands.push(json_command(crate::acp::catalog::BUILTIN_COMPACT, "Compact the session"));
    commands
}

fn json_command(name: &str, description: &str) -> Value {
    serde_json::json!({ "name": name, "description": description })
}

/// `resolveChange` (config-option.ts:77-101).
pub fn resolve_change(
    catalog: &Catalog,
    selection: &Selection,
    config_id: &str,
    value: &str,
) -> Result<Change, AcpError> {
    match config_id {
        "model" => {
            Ok(Change::Model(require_model(catalog, value, &current_model(catalog, selection))?))
        }
        "effort" => {
            let current = current_model(catalog, selection);
            let Some(model) = catalog.find_model(&current) else {
                return Err(AcpError::InvalidEffort { effort: value.to_owned() });
            };
            if value != DEFAULT_VARIANT_VALUE
                && !model.variants.iter().any(|variant| variant == value)
            {
                return Err(AcpError::InvalidEffort { effort: value.to_owned() });
            }
            Ok(Change::Model(ModelRef { variant: Some(value.to_owned()), ..current }))
        }
        "mode" => {
            let Some(mode) = catalog.modes.iter().find(|mode| mode.id == value) else {
                return Err(AcpError::InvalidMode { mode: value.to_owned() });
            };
            Ok(Change::Mode(mode.id.clone()))
        }
        other => Err(AcpError::InvalidConfigOption { config_id: other.to_owned() }),
    }
}

/// `parseModelSelection` (config-option.ts:103-122).
#[must_use]
pub fn parse_model_selection(value: &str, models: &[Model]) -> ModelRef {
    let mut providers: Vec<&str> = models.iter().map(|model| model.provider_id.as_str()).collect();
    providers.sort_unstable();
    let provider_id = providers
        .into_iter()
        .find(|provider| !provider.is_empty() && value.starts_with(&format!("{provider}/")));
    let Some(provider_id) = provider_id else {
        let Some(separator) = value.find('/') else {
            return ModelRef { provider_id: value.to_owned(), ..ModelRef::default() };
        };
        return ModelRef {
            provider_id: value[..separator].to_owned(),
            id: value[separator + 1..].to_owned(),
            ..ModelRef::default()
        };
    };
    let id = &value[provider_id.len() + 1..];
    if models.iter().any(|model| model.provider_id == provider_id && model.id == id) {
        return ModelRef {
            provider_id: provider_id.to_owned(),
            id: id.to_owned(),
            ..ModelRef::default()
        };
    }
    let Some(separator) = id.rfind('/') else {
        return ModelRef {
            provider_id: provider_id.to_owned(),
            id: id.to_owned(),
            ..ModelRef::default()
        };
    };
    let base_id = &id[..separator];
    let variant = &id[separator + 1..];
    let model = models.iter().find(|model| model.provider_id == provider_id && model.id == base_id);
    if let Some(model) = model {
        if !variant.is_empty() && model.variants.iter().any(|item| item == variant) {
            return ModelRef {
                provider_id: provider_id.to_owned(),
                id: base_id.to_owned(),
                variant: Some(variant.to_owned()),
            };
        }
    }
    ModelRef { provider_id: provider_id.to_owned(), id: id.to_owned(), ..ModelRef::default() }
}

/// `requireModel` (config-option.ts:124-138).
fn require_model(catalog: &Catalog, value: &str, current: &ModelRef) -> Result<ModelRef, AcpError> {
    let selected = parse_model_selection(value, &catalog.models);
    let Some(model) = catalog.find_model(&selected) else {
        return Err(AcpError::InvalidModel {
            model_id: value.to_owned(),
            provider_id: Some(selected.provider_id),
        });
    };
    let selected_variant = selected
        .variant
        .as_deref()
        .and_then(|variant| model.variants.iter().find(|item| item.as_str() == variant).cloned());
    if selected.variant.is_some() && selected_variant.is_none() {
        return Err(AcpError::InvalidEffort { effort: selected.variant.unwrap_or_default() });
    }
    let variant = selected_variant.or_else(|| {
        let keep = current.provider_id == model.provider_id
            && current.id == model.id
            && (current.variant.as_deref() == Some(DEFAULT_VARIANT_VALUE)
                || current
                    .variant
                    .as_ref()
                    .is_some_and(|variant| model.variants.iter().any(|item| item == variant)));
        keep.then(|| current.variant.clone()).flatten()
    });
    Ok(ModelRef { provider_id: model.provider_id.clone(), id: model.id.clone(), variant })
}

/// `selectVariant` (config-option.ts:140-145).
#[must_use]
fn select_variant(variant: Option<&str>, variants: &[String]) -> String {
    let Some(variant) = variant else { return DEFAULT_VARIANT_VALUE.to_owned() };
    if variant == DEFAULT_VARIANT_VALUE || variants.iter().any(|item| item == variant) {
        return variant.to_owned();
    }
    if variants.iter().any(|item| item == DEFAULT_VARIANT_VALUE) {
        return DEFAULT_VARIANT_VALUE.to_owned();
    }
    variants.first().cloned().unwrap_or_else(|| DEFAULT_VARIANT_VALUE.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::catalog::Mode;

    fn catalog() -> Catalog {
        Catalog {
            models: vec![
                Model {
                    provider_id: "a".to_owned(),
                    id: "small".to_owned(),
                    name: "A Small".to_owned(),
                    variants: vec![],
                    context_limit: 8192,
                },
                Model {
                    provider_id: "a".to_owned(),
                    id: "big".to_owned(),
                    name: "A Big".to_owned(),
                    variants: vec!["low".to_owned(), "high".to_owned()],
                    context_limit: 200_000,
                },
            ],
            default_model: ModelRef {
                provider_id: "a".to_owned(),
                id: "small".to_owned(),
                variant: None,
            },
            modes: vec![
                Mode { id: "build".to_owned(), name: "Build".to_owned(), description: None },
                Mode {
                    id: "plan".to_owned(),
                    name: "Plan".to_owned(),
                    description: Some("p".to_owned()),
                },
            ],
            default_mode_id: "build".to_owned(),
            commands: vec![],
        }
    }

    #[test]
    fn options_are_model_effort_mode() {
        let options = config_options(&catalog(), &Selection::default());
        assert_eq!(options.len(), 2, "no effort without variants");
        assert_eq!(options[0].id, agent_client_protocol::schema::v1::SessionConfigId::new("model"));
        assert_eq!(options[0].name, "Model");
        assert_eq!(options[1].id, agent_client_protocol::schema::v1::SessionConfigId::new("mode"));

        let selection = Selection {
            model: Some(ModelRef {
                provider_id: "a".to_owned(),
                id: "big".to_owned(),
                variant: Some("high".to_owned()),
            }),
            mode_id: None,
        };
        let options = config_options(&catalog(), &selection);
        assert_eq!(options.len(), 3);
        assert_eq!(
            options[1].id,
            agent_client_protocol::schema::v1::SessionConfigId::new("effort")
        );
        let agent_client_protocol::schema::v1::SessionConfigKind::Select(select) = &options[1].kind
        else {
            panic!("select");
        };
        assert_eq!(select.current_value.0.as_ref(), "high");
        let agent_client_protocol::schema::v1::SessionConfigKind::Select(model_select) =
            &options[0].kind
        else {
            panic!("select");
        };
        match &model_select.options {
            agent_client_protocol::schema::v1::SessionConfigSelectOptions::Ungrouped(options) => {
                assert_eq!(options[0].name, "a/A Big");
            }
            _ => panic!("ungrouped options"),
        }
    }

    #[test]
    fn model_changes_validate_against_the_catalog() {
        let change = resolve_change(&catalog(), &Selection::default(), "model", "a/big").unwrap();
        assert_eq!(
            change,
            Change::Model(ModelRef {
                provider_id: "a".to_owned(),
                id: "big".to_owned(),
                variant: None,
            })
        );
        let error =
            resolve_change(&catalog(), &Selection::default(), "model", "x/nope").unwrap_err();
        assert!(matches!(error, AcpError::InvalidModel { .. }));
    }

    #[test]
    fn variant_selection_parses_from_the_value() {
        let change =
            resolve_change(&catalog(), &Selection::default(), "model", "a/big/high").unwrap();
        assert_eq!(
            change,
            Change::Model(ModelRef {
                provider_id: "a".to_owned(),
                id: "big".to_owned(),
                variant: Some("high".to_owned())
            })
        );
        let error =
            resolve_change(&catalog(), &Selection::default(), "effort", "medium").unwrap_err();
        assert!(matches!(error, AcpError::InvalidEffort { .. }));
    }

    #[test]
    fn mode_changes_validate() {
        let change = resolve_change(&catalog(), &Selection::default(), "mode", "plan").unwrap();
        assert_eq!(change, Change::Mode("plan".to_owned()));
        let error = resolve_change(&catalog(), &Selection::default(), "mode", "nope").unwrap_err();
        assert!(matches!(error, AcpError::InvalidMode { .. }));
        let error =
            resolve_change(&catalog(), &Selection::default(), "temperature", "1").unwrap_err();
        assert!(matches!(error, AcpError::InvalidConfigOption { .. }));
    }

    #[test]
    fn unknown_providers_split_on_the_first_slash() {
        let parsed = parse_model_selection("zz/unknown/model", &catalog().models);
        assert_eq!(parsed.provider_id, "zz");
        assert_eq!(parsed.id, "unknown/model");
    }

    #[test]
    fn available_commands_include_builtin_compact() {
        let commands = available_commands(&catalog());
        assert_eq!(
            commands.last(),
            Some(&serde_json::json!({ "name": "compact", "description": "Compact the session" }))
        );
    }
}
