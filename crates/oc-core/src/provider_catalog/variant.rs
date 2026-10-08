//! Model variant overlays (port of `packages/core/src/variant.ts`).
//!
//! `PROTOCOLS[package]` maps a provider package onto a variant protocol: a
//! function over `(model, support)` producing named overlays (`none`,
//! `thinking`, `low`…`max`). [`resolve`] applies the v2 selection rules:
//! toggle variants contribute only their `none` entry, effort wins over
//! `budget_tokens`, and duplicates drop by first occurrence.

// Protocol switch arms stay parallel to the TS table even where several
// packages share a handler (mirroring the `PROTOCOLS` record), and budget
// math is JS-number parity.
#![allow(clippy::match_same_arms, clippy::cast_precision_loss)]

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use oc_schema::id::ModelVariantId;
use oc_schema::model::Variant;

use crate::provider_catalog::support::{Support, VariantModel};

// -- constants (variant.ts) ---------------------------------------------------

const EFFORTS: [&str; 3] = ["low", "medium", "high"];
const ENCRYPTED_REASONING: &str = "reasoning.encrypted_content";
const ANTHROPIC_OUTPUT_TOKEN_MAX: i64 = 32_000;
/// Alibaba thinking budget variants stay under 64k instead of reaching the
/// model's whole output limit.
const ALIBABA_THINKING_BUDGET_MAX: i64 = 64_000;

fn adaptive_thinking() -> Value {
    json!({"type": "adaptive", "display": "summarized"})
}

// -- overlay helpers ----------------------------------------------------------

/// `Omit<Variant, "id">` — settings/headers/body overlay.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overlay {
    pub settings: Option<Map<String, Value>>,
    pub headers: Option<BTreeMap<String, String>>,
    pub body: Option<BTreeMap<String, Value>>,
}

impl Overlay {
    fn settings(map: Map<String, Value>) -> Self {
        Self { settings: Some(map), headers: None, body: None }
    }

    /// `{ settings: { ...self.settings, key: value } }` spread.
    fn with_setting(mut self, key: &str, value: Value) -> Self {
        let map = self.settings.get_or_insert_with(Map::new);
        map.insert(key.to_string(), value);
        self
    }

    fn body(map: BTreeMap<String, Value>) -> Self {
        Self { settings: None, headers: None, body: Some(map) }
    }
}

fn settings_map(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

fn body_map(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

fn variant(id: &str, overlay: Overlay) -> Variant {
    Variant {
        id: ModelVariantId::parse(id),
        settings: overlay.settings.map(|rest| oc_schema::provider::Settings {
            rest: rest.into_iter().collect(),
            ..Default::default()
        }),
        headers: overlay.headers,
        body: overlay.body,
    }
}

fn efforts(values: &[String], spell: impl Fn(&str) -> Overlay) -> Vec<Variant> {
    values.iter().map(|effort| variant(effort, spell(effort))).collect()
}

fn toggle(off: Overlay, on: Overlay) -> Vec<Variant> {
    vec![variant("none", off), variant("thinking", on)]
}

/// Budget ladder: `high` ≈ half the ceiling, `max` at the ceiling (variant.ts
/// `budgets`). `ceiling` defaults to the model's output limit.
fn budgets(
    model: &VariantModel,
    support: &Support,
    spell: impl Fn(i64) -> Overlay,
    ceiling: i64,
) -> Vec<Variant> {
    let (min, max) = match support {
        Support::BudgetTokens { min, max } => (*min, *max),
        _ => return vec![],
    };
    let maximum = max.unwrap_or(ceiling - 1).min(model.limit_output - 1).min(ceiling - 1);
    if maximum <= 0 {
        return vec![];
    }
    let half = (maximum + 1) / 2;
    let high = min.unwrap_or(0).max(half).min(maximum);
    vec![variant("high", spell(high)), variant("max", spell(maximum))]
}

// -- claude family parsing ----------------------------------------------------

struct ClaudeInfo {
    family: Option<String>,
    major: Option<i64>,
    minor: Option<i64>,
    manual: bool,
    always: bool,
}

static CLAUDE_FAMILY_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:claude-)?(opus|sonnet|haiku|fable|mythos)-(\d+)(?:[.-](\d+))?").unwrap()
});
static CLAUDE_VERSION_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)claude-(\d+)(?:[.-](\d+))?-(opus|sonnet|haiku|fable|mythos)").unwrap()
});

fn number(group: Option<regex::Match<'_>>) -> Option<i64> {
    group.and_then(|m| m.as_str().parse::<i64>().ok())
}

fn claude_info(model: &VariantModel) -> ClaudeInfo {
    let id = &model.model_id;
    let family_first = CLAUDE_FAMILY_FIRST.captures(id);
    let version_first = CLAUDE_VERSION_FIRST.captures(id);
    let family = family_first
        .as_ref()
        .map(|c| c.get(1).map(|m| m.as_str().to_lowercase()))
        .or_else(|| version_first.as_ref().map(|c| c.get(3).map(|m| m.as_str().to_lowercase())))
        .flatten();
    let major = number(family_first.as_ref().and_then(|c| c.get(2)))
        .or_else(|| number(version_first.as_ref().and_then(|c| c.get(1))));
    let minor = number(family_first.as_ref().and_then(|c| c.get(3)))
        .or_else(|| number(version_first.as_ref().and_then(|c| c.get(2))))
        .or(Some(0));
    let (major, minor) = match (major, minor) {
        (Some(major), Some(minor)) => (Some(major), minor),
        _ => (None, 0),
    };
    let manual = (major == Some(3) && minor == 7) || (major == Some(4) && minor < 6);
    let always = matches!(family.as_deref(), Some("fable" | "mythos"))
        || id.to_lowercase().contains("mythos-preview");
    ClaudeInfo { family, major, minor: Some(minor), manual, always }
}

fn manual_thinking(model: &VariantModel) -> Option<Overlay> {
    let tokens = 16_000.min(model.limit_output - 1);
    if tokens < 1024 {
        return None;
    }
    Some(Overlay::settings(settings_map(&[(
        "thinking",
        json!({"type": "enabled", "budgetTokens": tokens}),
    )])))
}

// -- protocol functions -------------------------------------------------------

type ProtocolFn = fn(&VariantModel, &Support) -> Vec<Variant>;

fn openai_chat(_model: &VariantModel, support: &Support) -> Vec<Variant> {
    let Support::Effort { values } = support else {
        return vec![];
    };
    let defaults = EFFORTS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    let list = values.clone().unwrap_or(defaults);
    efforts(&list, |effort| Overlay::settings(settings_map(&[("reasoningEffort", json!(effort))])))
}

fn responses_effort(effort: &str) -> Overlay {
    Overlay::settings(settings_map(&[
        ("reasoningEffort", json!(effort)),
        ("reasoningSummary", json!("auto")),
        ("include", json!([ENCRYPTED_REASONING])),
    ]))
}

fn openai_responses(_model: &VariantModel, support: &Support) -> Vec<Variant> {
    let Support::Effort { values } = support else {
        return vec![];
    };
    let defaults = ["none", "minimal", "low", "medium", "high", "xhigh"]
        .iter()
        .map(|s| (*s).to_string())
        .collect::<Vec<_>>();
    let list = values.clone().unwrap_or(defaults);
    efforts(&list, responses_effort)
}

fn xai_responses(_model: &VariantModel, support: &Support) -> Vec<Variant> {
    let Support::Effort { values } = support else {
        return vec![];
    };
    let defaults = EFFORTS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    efforts(&values.clone().unwrap_or(defaults), responses_effort)
}

fn cloudflare_ai_gateway(model: &VariantModel, support: &Support) -> Vec<Variant> {
    if model.model_id.starts_with("openai/") {
        openai_responses(model, support)
    } else if model.model_id.starts_with("anthropic/") {
        anthropic_messages(model, support)
    } else {
        openai_chat(model, support)
    }
}

fn deepseek_chat(_model: &VariantModel, support: &Support) -> Vec<Variant> {
    match support {
        Support::Effort { values } => {
            let defaults = ["low", "high", "max"].iter().map(|s| (*s).to_string()).collect();
            efforts(&values.clone().unwrap_or(defaults), |effort| {
                if effort == "none" {
                    Overlay::body(body_map(&[("thinking", json!({"type": "disabled"}))]))
                } else {
                    Overlay::settings(settings_map(&[("reasoningEffort", json!(effort))]))
                        .merge_body(body_map(&[("thinking", json!({"type": "enabled"}))]))
                }
            })
        }
        Support::Toggle => toggle(
            Overlay::body(body_map(&[("thinking", json!({"type": "disabled"}))])),
            Overlay::body(body_map(&[("thinking", json!({"type": "enabled"}))])),
        ),
        Support::BudgetTokens { .. } => vec![],
    }
}

fn nvidia_template(pairs: &[(&str, Value)]) -> Overlay {
    Overlay::body(body_map(&[("chat_template_kwargs", Value::Object(settings_map(pairs)))]))
}

fn nvidia_toggle(key: &str) -> Vec<Variant> {
    toggle(nvidia_template(&[(key, json!(false))]), nvidia_template(&[(key, json!(true))]))
}

fn nvidia_chat(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let id = model.model_id.to_lowercase();
    if id.contains("deepseek-v4") {
        return match support {
            Support::Toggle => nvidia_toggle("thinking"),
            Support::Effort { values } => {
                let defaults = ["none", "high", "max"].iter().map(|s| (*s).to_string()).collect();
                efforts(&values.clone().unwrap_or(defaults), |effort| {
                    if effort == "none" {
                        nvidia_template(&[("thinking", json!(false))])
                    } else {
                        nvidia_template(&[
                            ("thinking", json!(true)),
                            ("reasoning_effort", json!(effort)),
                        ])
                    }
                })
            }
            Support::BudgetTokens { .. } => vec![],
        };
    }
    if id.contains("kimi-k2.6") || id.contains("kimi-k2-6") {
        return nvidia_toggle("thinking");
    }
    if id.contains("kimi-k3") {
        return match support {
            Support::Toggle => nvidia_toggle("thinking"),
            Support::Effort { values } => {
                let defaults = ["low", "high", "max"].iter().map(|s| (*s).to_string()).collect();
                efforts(&values.clone().unwrap_or(defaults), |effort| {
                    Overlay::settings(settings_map(&[("reasoningEffort", json!(effort))]))
                })
            }
            Support::BudgetTokens { .. } => vec![],
        };
    }
    if id.contains("minimax-m3") {
        return toggle(
            nvidia_template(&[("thinking_mode", json!("disabled"))]),
            nvidia_template(&[("thinking_mode", json!("enabled"))]),
        );
    }
    if id.contains("glm") {
        return toggle(
            nvidia_template(&[("enable_thinking", json!(false))]),
            nvidia_template(&[("enable_thinking", json!(true)), ("clear_thinking", json!(false))]),
        );
    }
    let toggle_only = id.contains("qwen") || id.contains("gemma") || id.contains("nemotron");
    match support {
        Support::Effort { values } => {
            if toggle_only && values.is_none() {
                nvidia_toggle("enable_thinking")
            } else {
                openai_chat(model, support)
            }
        }
        Support::Toggle => nvidia_toggle("enable_thinking"),
        Support::BudgetTokens { .. } => budgets(
            model,
            support,
            |tokens| {
                Overlay::body(body_map(&[
                    ("chat_template_kwargs", json!({"enable_thinking": true})),
                    ("reasoning_budget", json!(tokens)),
                ]))
            },
            model.limit_output,
        ),
    }
}

fn workers_ai_template(pairs: &[(&str, Value)]) -> Overlay {
    Overlay::body(body_map(&[("chat_template_kwargs", Value::Object(settings_map(pairs)))]))
}

fn workers_ai_chat(_model: &VariantModel, support: &Support) -> Vec<Variant> {
    let defaults = EFFORTS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    match support {
        Support::Effort { values } => efforts(&values.clone().unwrap_or(defaults), |effort| {
            if effort == "none" {
                workers_ai_template(&[
                    ("enable_thinking", json!(false)),
                    ("thinking", json!(false)),
                ])
            } else {
                Overlay::settings(settings_map(&[("reasoningEffort", json!(effort))]))
            }
        }),
        Support::Toggle => toggle(
            workers_ai_template(&[("enable_thinking", json!(false)), ("thinking", json!(false))]),
            workers_ai_template(&[("enable_thinking", json!(true)), ("thinking", json!(true))]),
        ),
        Support::BudgetTokens { .. } => vec![],
    }
}

fn baseten_template(enable_thinking: bool) -> Overlay {
    Overlay::body(body_map(&[(
        "chat_template_args",
        json!({ "enable_thinking": enable_thinking }),
    )]))
}

fn baseten_chat(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let id = model.model_id.to_lowercase();
    match support {
        Support::Effort { values } => {
            let defaults = EFFORTS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
            efforts(&values.clone().unwrap_or(defaults), |effort| {
                let mut overlay =
                    Overlay::settings(settings_map(&[("reasoningEffort", json!(effort))]));
                let glm =
                    id.contains("glm-5.2") || id.contains("glm-5-2") || id.contains("glm-5p2");
                if glm {
                    overlay = overlay
                        .merge_body(baseten_template(effort != "none").body.unwrap_or_default());
                }
                if id.contains("deepseek-v4-pro-0813") && effort != "none" {
                    overlay =
                        overlay.merge_body(body_map(&[("thinking", json!({"type": "enabled"}))]));
                }
                overlay
            })
        }
        Support::Toggle => toggle(baseten_template(false), baseten_template(true)),
        Support::BudgetTokens { .. } => vec![],
    }
}

fn deepinfra_chat(model: &VariantModel, support: &Support) -> Vec<Variant> {
    if Regex::new(r"(?i)kimi[-.]?k2[.-]7-code").unwrap().is_match(&model.model_id) {
        return vec![];
    }
    let defaults = EFFORTS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    match support {
        Support::Effort { values } => efforts(&values.clone().unwrap_or(defaults), |effort| {
            Overlay::settings(settings_map(&[("reasoningEffort", json!(effort))]))
        }),
        Support::Toggle => toggle(
            Overlay::body(body_map(&[("reasoning", json!({"enabled": false}))])),
            Overlay::body(body_map(&[("reasoning", json!({"enabled": true}))])),
        ),
        Support::BudgetTokens { .. } => vec![],
    }
}

fn openai_compatible(model: &VariantModel, support: &Support) -> Vec<Variant> {
    if model.provider_id == "nvidia" {
        nvidia_chat(model, support)
    } else {
        openai_chat(model, support)
    }
}

fn venice_chat(_model: &VariantModel, support: &Support) -> Vec<Variant> {
    let defaults = EFFORTS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    match support {
        Support::Effort { values } => efforts(&values.clone().unwrap_or(defaults), |effort| {
            Overlay::settings(settings_map(&[("reasoningEffort", json!(effort))]))
        }),
        Support::Toggle => toggle(
            Overlay::settings(settings_map(&[("reasoning", json!({"enabled": false}))])),
            Overlay::settings(settings_map(&[("reasoning", json!({"enabled": true}))])),
        ),
        Support::BudgetTokens { .. } => vec![],
    }
}

fn moonshot_chat(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let id = model.model_id.to_lowercase();
    let toggleable =
        id.contains("k2.5") || id.contains("k2-5") || id.contains("k2.6") || id.contains("k2-6");
    let fixed = id.contains("k2-thinking") || id.contains("k2.7-code") || id.contains("k2-7-code");
    if fixed {
        return vec![];
    }
    if toggleable
        && (matches!(support, Support::Toggle)
            || matches!(support, Support::Effort { values: None }))
    {
        return toggle(
            Overlay::settings(settings_map(&[("thinking", json!({"type": "disabled"}))])),
            Overlay::settings(settings_map(&[("thinking", json!({"type": "enabled"}))])),
        );
    }
    if toggleable || !matches!(support, Support::Effort { .. }) {
        return vec![];
    }
    let Support::Effort { values } = support else {
        return vec![];
    };
    let defaults = ["low", "high", "max"].iter().map(|s| (*s).to_string()).collect();
    efforts(&values.clone().unwrap_or(defaults), |effort| {
        Overlay::settings(settings_map(&[("reasoningEffort", json!(effort))]))
    })
}

fn alibaba_chat(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let id = model.model_id.to_lowercase();
    let fixed = id.contains("-thinking")
        || id.contains("qwq")
        || id.contains("deepseek-r1")
        || id.contains("minimax-m2");
    match support {
        Support::Effort { values } => {
            if fixed && values.is_none() {
                return vec![];
            }
            let hosted = id.contains("glm") || id.contains("deepseek");
            let defaults: Vec<String> = if hosted {
                ["high", "max"].as_slice()
            } else {
                ["low", "medium", "xhigh"].as_slice()
            }
            .iter()
            .map(|s| (*s).to_string())
            .collect();
            efforts(values.as_ref().unwrap_or(&defaults), |effort| {
                if effort == "none" {
                    Overlay::settings(settings_map(&[("enableThinking", json!(false))]))
                } else {
                    Overlay::settings(settings_map(&[
                        ("enableThinking", json!(true)),
                        ("reasoningEffort", json!(effort)),
                    ]))
                }
            })
        }
        Support::Toggle => toggle(
            Overlay::settings(settings_map(&[("enableThinking", json!(false))])),
            Overlay::settings(settings_map(&[("enableThinking", json!(true))])),
        ),
        Support::BudgetTokens { .. } => budgets(
            model,
            support,
            |tokens| {
                Overlay::settings(settings_map(&[
                    ("enableThinking", json!(true)),
                    ("thinkingBudget", json!(tokens)),
                ]))
            },
            ALIBABA_THINKING_BUDGET_MAX,
        ),
    }
}

fn zai_chat(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let id = model.model_id.to_lowercase();
    let version = Regex::new(r"glm-?(\d+)(?:(?:[.-]|p)(\d+))?").unwrap().captures(&id);
    let major =
        version.as_ref().and_then(|c| c.get(1)).and_then(|m| m.as_str().parse::<i64>().ok());
    let minor =
        version.as_ref().and_then(|c| c.get(2)).and_then(|m| m.as_str().parse::<i64>().ok());
    let coding = model.package.as_deref().is_some_and(|p| p.contains("zai-coding-plan"));
    let latest = coding
        || major.is_none()
        || major.is_some_and(|m| m > 5)
        || (major == Some(5) && minor.unwrap_or(0) >= 3);
    let effort_only = major == Some(5) && minor == Some(2);
    if matches!(support, Support::Toggle)
        || (!latest && !effort_only && matches!(support, Support::Effort { values: None }))
    {
        if latest {
            return vec![];
        }
        return toggle(
            Overlay::settings(settings_map(&[("thinking", json!({"type": "disabled"}))])),
            Overlay::settings(settings_map(&[(
                "thinking",
                json!({"type": "enabled", "clear_thinking": false}),
            )])),
        );
    }
    let Support::Effort { values } = support else {
        return vec![];
    };
    let defaults: Vec<String> =
        if latest { ["low", "high", "max"].as_slice() } else { ["high", "max"].as_slice() }
            .iter()
            .map(|s| (*s).to_string())
            .collect();
    efforts(values.as_ref().unwrap_or(&defaults), |value| {
        if value == "none" || value == "minimal" {
            Overlay::settings(settings_map(&[("thinking", json!({"type": "disabled"}))]))
        } else {
            Overlay::settings(settings_map(&[
                ("thinking", json!({"type": "enabled", "clear_thinking": false})),
                ("reasoningEffort", json!(value)),
            ]))
        }
    })
}

fn anthropic_messages(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let info = claude_info(model);
    let opus45 =
        info.family.as_deref() == Some("opus") && info.major == Some(4) && info.minor == Some(5);
    match support {
        Support::Effort { values } => {
            if info.manual && !opus45 {
                return anthropic_messages(
                    model,
                    &Support::BudgetTokens { min: Some(1024), max: None },
                );
            }
            let thinking = if opus45 {
                manual_thinking(model)
            } else {
                Some(Overlay::settings(settings_map(&[("thinking", adaptive_thinking())])))
            };
            let Some(thinking) = thinking else {
                return vec![];
            };
            let defaults: Vec<String> = if info.major == Some(4) && info.minor == Some(6) {
                EFFORTS.iter().chain(["max"].iter()).map(|s| (*s).to_string()).collect()
            } else {
                EFFORTS.iter().chain(["xhigh", "max"].iter()).map(|s| (*s).to_string()).collect()
            };
            let list = values.clone().unwrap_or(defaults);
            let list = list
                .into_iter()
                .filter(|effort| !info.always || effort != "none")
                .collect::<Vec<_>>();
            efforts(&list, move |effort| {
                if effort == "none" {
                    Overlay::settings(settings_map(&[("thinking", json!({"type": "disabled"}))]))
                } else {
                    thinking.clone().with_setting("effort", json!(effort))
                }
            })
        }
        Support::Toggle => {
            if info.always {
                return vec![];
            }
            let thinking = if info.manual {
                manual_thinking(model)
            } else {
                Some(Overlay::settings(settings_map(&[("thinking", adaptive_thinking())])))
            };
            let Some(thinking) = thinking else {
                return vec![];
            };
            toggle(
                Overlay::settings(settings_map(&[("thinking", json!({"type": "disabled"}))])),
                thinking,
            )
        }
        Support::BudgetTokens { .. } => budgets(
            model,
            support,
            |tokens| {
                Overlay::settings(settings_map(&[(
                    "thinking",
                    json!({"type": "enabled", "budgetTokens": tokens}),
                )]))
            },
            ANTHROPIC_OUTPUT_TOKEN_MAX,
        ),
    }
}

fn minimax_messages(model: &VariantModel, support: &Support) -> Vec<Variant> {
    if Regex::new(r"(?i)minimax[-.]?m2(?:[.-]|$)").unwrap().is_match(&model.model_id) {
        return vec![];
    }
    let configurable =
        matches!(support, Support::Toggle) || matches!(support, Support::Effort { values: None });
    if !configurable {
        return vec![];
    }
    toggle(
        Overlay::settings(settings_map(&[("thinking", json!({"type": "disabled"}))])),
        Overlay::settings(settings_map(&[("thinking", json!({"type": "adaptive"}))])),
    )
}

fn moonshot_messages(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let Support::Effort { values } = support else {
        return vec![];
    };
    if Regex::new(r"(?i)kimi[-.]?k2").unwrap().is_match(&model.model_id) {
        return vec![];
    }
    let defaults = ["low", "high", "max"].iter().map(|s| (*s).to_string()).collect();
    efforts(&values.clone().unwrap_or(defaults), |effort| {
        Overlay::settings(settings_map(&[("effort", json!(effort))]))
    })
}

fn alibaba_messages(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let id = model.model_id.to_lowercase();
    match support {
        Support::Effort { values } => {
            let hosted = id.contains("glm") || id.contains("deepseek");
            let defaults: Vec<String> = if hosted {
                ["high", "max"].as_slice()
            } else {
                ["low", "medium", "xhigh"].as_slice()
            }
            .iter()
            .map(|s| (*s).to_string())
            .collect();
            efforts(values.as_ref().unwrap_or(&defaults), |effort| {
                if effort == "none" {
                    Overlay::settings(settings_map(&[("thinking", json!({"type": "disabled"}))]))
                } else {
                    Overlay::settings(settings_map(&[
                        ("thinking", json!({"type": "enabled"})),
                        ("effort", json!(effort)),
                    ]))
                }
            })
        }
        Support::Toggle => toggle(
            Overlay::settings(settings_map(&[("thinking", json!({"type": "disabled"}))])),
            Overlay::settings(settings_map(&[("thinking", json!({"type": "enabled"}))])),
        ),
        Support::BudgetTokens { .. } => budgets(
            model,
            support,
            |tokens| {
                Overlay::settings(settings_map(&[(
                    "thinking",
                    json!({"type": "enabled", "budgetTokens": tokens}),
                )]))
            },
            ALIBABA_THINKING_BUDGET_MAX,
        ),
    }
}

fn zai_messages(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let id = model.model_id.to_lowercase();
    let forced = id.contains("glm-5.3") || id.contains("glm-5-3") || id.contains("glm-5p3");
    match support {
        Support::Effort { values } => {
            let defaults: Vec<String> =
                if forced { ["low", "high", "max"].as_slice() } else { ["high", "max"].as_slice() }
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect();
            efforts(values.as_ref().unwrap_or(&defaults), |effort| {
                Overlay::settings(settings_map(&[
                    (
                        "thinking",
                        json!({"type": if effort == "none" || effort == "minimal" { "disabled" } else { "enabled" }}),
                    ),
                    ("effort", json!(effort)),
                ]))
            })
        }
        Support::Toggle => {
            if forced {
                vec![]
            } else {
                toggle(
                    Overlay::settings(settings_map(&[("thinking", json!({"type": "disabled"}))])),
                    Overlay::settings(settings_map(&[("thinking", json!({"type": "enabled"}))])),
                )
            }
        }
        Support::BudgetTokens { .. } => vec![],
    }
}

fn gemini(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let defaults = EFFORTS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    match support {
        Support::Effort { values } => efforts(&values.clone().unwrap_or(defaults), |effort| {
            Overlay::settings(settings_map(&[(
                "thinkingConfig",
                json!({"includeThoughts": true, "thinkingLevel": effort}),
            )]))
        }),
        Support::Toggle => toggle(
            Overlay::settings(settings_map(&[(
                "thinkingConfig",
                json!({"includeThoughts": false, "thinkingBudget": 0}),
            )])),
            Overlay::settings(settings_map(&[(
                "thinkingConfig",
                json!({"includeThoughts": true, "thinkingBudget": -1}),
            )])),
        ),
        Support::BudgetTokens { .. } => budgets(
            model,
            support,
            |tokens| {
                Overlay::settings(settings_map(&[(
                    "thinkingConfig",
                    json!({"includeThoughts": true, "thinkingBudget": tokens}),
                )]))
            },
            model.limit_output,
        ),
    }
}

fn openrouter(model: &VariantModel, support: &Support) -> Vec<Variant> {
    match support {
        Support::Effort { values } => {
            let defaults = EFFORTS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
            efforts(&values.clone().unwrap_or(defaults), |effort| {
                Overlay::settings(settings_map(&[("reasoning", json!({ "effort": effort }))]))
            })
        }
        Support::Toggle => toggle(
            Overlay::settings(settings_map(&[("reasoning", json!({"enabled": false}))])),
            Overlay::settings(settings_map(&[("reasoning", json!({"enabled": true}))])),
        ),
        Support::BudgetTokens { .. } => budgets(
            model,
            support,
            |tokens| {
                Overlay::settings(settings_map(&[("reasoning", json!({ "max_tokens": tokens }))]))
            },
            model.limit_output,
        ),
    }
}

fn bedrock_converse(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let id = &model.model_id;
    let claude = id.contains("anthropic");
    let fields = |pairs: &[(&str, Value)]| {
        Overlay::body(body_map(&[(
            "additionalModelRequestFields",
            Value::Object(settings_map(pairs)),
        )]))
    };
    let defaults = EFFORTS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    match support {
        Support::Effort { values } => efforts(&values.clone().unwrap_or(defaults), |effort| {
            if claude {
                let mut pairs: Vec<(&str, Value)> =
                    vec![("output_config", json!({ "effort": effort }))];
                if !claude_info(model).manual {
                    pairs.insert(0, ("thinking", adaptive_thinking()));
                }
                fields(&pairs)
            } else if id.contains("openai.gpt-oss") {
                fields(&[("reasoning_effort", json!(effort))])
            } else if id.contains("openai.") || id.contains("xai.") {
                fields(&[("reasoning", json!({ "effort": effort }))])
            } else {
                fields(&[(
                    "reasoningConfig",
                    json!({"type": "enabled", "maxReasoningEffort": effort}),
                )])
            }
        }),
        Support::Toggle => {
            if claude {
                toggle(
                    fields(&[("thinking", json!({"type": "disabled"}))]),
                    fields(&[("thinking", adaptive_thinking())]),
                )
            } else {
                toggle(
                    fields(&[("reasoningConfig", json!({"type": "disabled"}))]),
                    fields(&[("reasoningConfig", json!({"type": "enabled"}))]),
                )
            }
        }
        Support::BudgetTokens { .. } => {
            let ceiling = if claude { ANTHROPIC_OUTPUT_TOKEN_MAX } else { model.limit_output };
            budgets(
                model,
                support,
                |tokens| {
                    if claude {
                        Overlay::settings(settings_map(&[(
                            "thinking",
                            json!({"type": "enabled", "budgetTokens": tokens}),
                        )]))
                    } else {
                        fields(&[(
                            "reasoningConfig",
                            json!({"type": "enabled", "budgetTokens": tokens}),
                        )])
                    }
                },
                ceiling,
            )
        }
    }
}

fn cohere(model: &VariantModel, support: &Support) -> Vec<Variant> {
    match support {
        Support::Effort { .. } => vec![],
        Support::Toggle => toggle(
            Overlay::settings(settings_map(&[("thinking", json!({"type": "disabled"}))])),
            Overlay::settings(settings_map(&[("thinking", json!({"type": "enabled"}))])),
        ),
        Support::BudgetTokens { .. } => budgets(
            model,
            support,
            |tokens| {
                Overlay::settings(settings_map(&[(
                    "thinking",
                    json!({"type": "enabled", "tokenBudget": tokens}),
                )]))
            },
            model.limit_output,
        ),
    }
}

fn vercel_gateway(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let id = &model.model_id;
    if id.starts_with("openai/gpt-") {
        return openai_responses(model, support);
    }
    if id.starts_with("spacexai/grok-") {
        return xai_responses(model, support);
    }
    if id.starts_with("anthropic/") {
        return anthropic_messages(model, support);
    }
    match support {
        Support::Effort { .. } => openai_chat(model, support),
        Support::Toggle => toggle(
            Overlay::settings(settings_map(&[("thinking", json!({"type": "disabled"}))])),
            Overlay::settings(settings_map(&[("thinking", json!({"type": "adaptive"}))])),
        ),
        Support::BudgetTokens { .. } => {
            let ceiling = if id.starts_with("alibaba/") {
                ALIBABA_THINKING_BUDGET_MAX
            } else {
                model.limit_output
            };
            budgets(
                model,
                support,
                |tokens| {
                    Overlay::settings(settings_map(&[(
                        "thinking",
                        json!({"type": "enabled", "budgetTokens": tokens}),
                    )]))
                },
                ceiling,
            )
        }
    }
}

// One function per TS protocol, kept whole for diff-ability against
// variant.ts (119 lines vs 100 budget).
#[allow(clippy::too_many_lines)]
fn sap_ai_core(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let id = &model.model_id;
    let sap = |pairs: &[(&str, Value)]| {
        Overlay::settings(settings_map(&[("modelParams", Value::Object(settings_map(pairs)))]))
    };
    let defaults = EFFORTS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    match support {
        Support::Effort { values } => efforts(&values.clone().unwrap_or(defaults), |effort| {
            if id.contains("anthropic") {
                let mut pairs: Vec<(&str, Value)> =
                    vec![("output_config", json!({ "effort": effort }))];
                if !claude_info(model).manual {
                    pairs.insert(0, ("thinking", adaptive_thinking()));
                }
                sap(&[("additionalModelRequestFields", Value::Object(settings_map(&pairs)))])
            } else if id.contains("gemini") {
                sap(&[(
                    "thinkingConfig",
                    json!({"includeThoughts": true, "thinkingLevel": effort}),
                )])
            } else if id.contains("amazon--nova") {
                sap(&[(
                    "additionalModelRequestFields",
                    json!({ "output_config": { "effort": effort } }),
                )])
            } else {
                sap(&[("reasoning_effort", json!(effort))])
            }
        }),
        Support::Toggle => {
            if id.contains("gemini") {
                toggle(
                    sap(&[(
                        "thinkingConfig",
                        json!({"includeThoughts": false, "thinkingBudget": 0}),
                    )]),
                    sap(&[(
                        "thinkingConfig",
                        json!({"includeThoughts": true, "thinkingBudget": -1}),
                    )]),
                )
            } else if id.contains("cohere") {
                toggle(
                    sap(&[("thinking", json!({"type": "disabled"}))]),
                    sap(&[("thinking", json!({"type": "enabled"}))]),
                )
            } else if id.contains("amazon--nova") {
                toggle(
                    sap(&[(
                        "additionalModelRequestFields",
                        json!({"thinking": {"type": "disabled"}}),
                    )]),
                    sap(&[(
                        "additionalModelRequestFields",
                        json!({"thinking": {"type": "enabled"}}),
                    )]),
                )
            } else if id.contains("anthropic") {
                toggle(
                    sap(&[(
                        "additionalModelRequestFields",
                        json!({"thinking": {"type": "disabled"}}),
                    )]),
                    sap(&[(
                        "additionalModelRequestFields",
                        Value::Object(settings_map(&[("thinking", adaptive_thinking())])),
                    )]),
                )
            } else {
                vec![]
            }
        }
        Support::BudgetTokens { .. } => {
            if id.contains("anthropic") {
                budgets(
                    model,
                    support,
                    |tokens| {
                        sap(&[(
                            "additionalModelRequestFields",
                            json!({"thinking": {"type": "enabled", "budget_tokens": tokens}}),
                        )])
                    },
                    ANTHROPIC_OUTPUT_TOKEN_MAX,
                )
            } else if id.contains("gemini") {
                budgets(
                    model,
                    support,
                    |tokens| {
                        sap(&[(
                            "thinkingConfig",
                            json!({"includeThoughts": true, "thinkingBudget": tokens}),
                        )])
                    },
                    model.limit_output,
                )
            } else if id.contains("cohere") {
                budgets(
                    model,
                    support,
                    |tokens| {
                        sap(&[("thinking", json!({"type": "enabled", "token_budget": tokens}))])
                    },
                    model.limit_output,
                )
            } else {
                vec![]
            }
        }
    }
}

fn gitlab_anthropic(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let info = claude_info(model);
    let disabled = || Overlay::settings(settings_map(&[("thinking", json!({"type": "disabled"}))]));
    let adaptive = |effort: &str| {
        Overlay::settings(settings_map(&[(
            "thinking",
            json!({"type": "adaptive", "effort": effort}),
        )]))
    };
    match support {
        Support::Effort { values } => {
            if info.manual {
                return gitlab_anthropic(
                    model,
                    &Support::BudgetTokens { min: Some(1024), max: None },
                );
            }
            let defaults: Vec<String> = if info.major == Some(4) && info.minor == Some(6) {
                EFFORTS.iter().chain(["max"].iter()).map(|s| (*s).to_string()).collect()
            } else {
                EFFORTS.iter().chain(["xhigh", "max"].iter()).map(|s| (*s).to_string()).collect()
            };
            let list = values.clone().unwrap_or(defaults);
            efforts(&list, |effort| if effort == "none" { disabled() } else { adaptive(effort) })
        }
        Support::Toggle => {
            if info.always {
                return vec![];
            }
            let thinking =
                if info.manual { manual_thinking(model) } else { Some(adaptive("high")) };
            let Some(thinking) = thinking else {
                return vec![];
            };
            toggle(disabled(), thinking)
        }
        Support::BudgetTokens { .. } => budgets(
            model,
            support,
            |tokens| {
                Overlay::settings(settings_map(&[(
                    "thinking",
                    json!({"type": "enabled", "budgetTokens": tokens}),
                )]))
            },
            ANTHROPIC_OUTPUT_TOKEN_MAX,
        ),
    }
}

fn gitlab(model: &VariantModel, support: &Support) -> Vec<Variant> {
    let id = model.model_id.to_lowercase();
    if id.starts_with("duo-workflow") {
        return vec![];
    }
    if claude_info(model).family.is_some() {
        return gitlab_anthropic(model, support);
    }
    if id.contains("gpt") || id.contains("codex") {
        return openai_chat(model, support);
    }
    vec![]
}

// -- the PROTOCOLS table ------------------------------------------------------

/// v2 `PROTOCOLS[package]`.
#[must_use]
pub fn protocol(package: &str) -> Option<ProtocolFn> {
    Some(match package {
        "@opencode/ai/providers/openai" => openai_responses,
        "@opencode/ai/providers/azure/responses" => openai_responses,
        "@opencode/ai/providers/amazon-bedrock/mantle/chat" => openai_responses,
        "@opencode/ai/providers/amazon-bedrock/mantle/responses" => openai_responses,
        "@opencode/ai/providers/alibaba/responses" => openai_responses,
        "@opencode/ai/providers/meta/responses" => openai_responses,
        "@opencode/ai/providers/minimax/responses" => openai_responses,
        "@opencode/ai/providers/moonshot/responses" => openai_responses,
        "@opencode/ai/providers/zai-coding-plan/responses" => openai_responses,

        "@opencode/ai/providers/openai-compatible" => openai_compatible,
        "@opencode/ai/providers/azure/chat" => openai_chat,
        "@opencode/ai/providers/google-vertex/chat" => openai_chat,
        "@opencode/ai/providers/alibaba/chat" => alibaba_chat,
        "@opencode/ai/providers/baseten" => baseten_chat,
        "@opencode/ai/providers/cerebras" => openai_chat,
        "@opencode/ai/providers/cloudflare-workers-ai" => workers_ai_chat,
        "@opencode/ai/providers/cohere/chat" => openai_chat,
        "@opencode/ai/providers/deepinfra" => deepinfra_chat,
        "@opencode/ai/providers/deepseek" => deepseek_chat,
        "@opencode/ai/providers/digitalocean" => openai_chat,
        "@opencode/ai/providers/fireworks" => openai_chat,
        "@opencode/ai/providers/groq" => openai_chat,
        "@opencode/ai/providers/meta/chat" => openai_chat,
        "@opencode/ai/providers/minimax/chat" => openai_chat,
        "@opencode/ai/providers/mistral" => openai_chat,
        "@opencode/ai/providers/moonshot/chat" => moonshot_chat,
        "@opencode/ai/providers/togetherai" => openai_chat,
        "@opencode/ai/providers/venice" => venice_chat,
        "@opencode/ai/providers/xai" => xai_responses,
        "@opencode/ai/providers/zai/chat" => zai_chat,
        "@opencode/ai/providers/zai-coding-plan/chat" => zai_chat,

        "@opencode/ai/providers/anthropic" => anthropic_messages,
        "@opencode/ai/providers/google-vertex/messages" => anthropic_messages,
        "@opencode/ai/providers/alibaba/messages" => alibaba_messages,
        "@opencode/ai/providers/meta/messages" => anthropic_messages,
        "@opencode/ai/providers/minimax/messages" => minimax_messages,
        "@opencode/ai/providers/moonshot/messages" => moonshot_messages,
        "@opencode/ai/providers/zai-coding-plan/messages" => zai_messages,

        "@opencode/ai/providers/google" => gemini,
        "@opencode/ai/providers/google-vertex" => gemini,

        "@opencode/ai/providers/amazon-bedrock" => bedrock_converse,
        "@opencode/ai/providers/cohere" => cohere,
        "@opencode/ai/providers/openrouter" => openrouter,

        "aisdk:venice-ai-sdk-provider" => venice_chat,
        "@opencode/ai/providers/cloudflare-ai-gateway" => cloudflare_ai_gateway,
        "@opencode/ai/providers/vercel-ai-gateway" => vercel_gateway,
        "aisdk:@ai-sdk/gateway" => vercel_gateway,
        "aisdk:@jerome-benoit/sap-ai-provider-v2" => sap_ai_core,
        "aisdk:gitlab-ai-provider" => gitlab,
        _ => return None,
    })
}

/// v2 `Variant.resolve`: toggle variants contribute only their `none` entry;
/// the effort support wins over `budget_tokens`; duplicates drop by first
/// occurrence. Default supports (when the model declares none) is a single
/// unparameterized effort.
pub fn resolve(model: &VariantModel, supports: &[Support]) -> Vec<Variant> {
    let Some(protocol) = model.package.as_deref().and_then(protocol) else {
        return vec![];
    };
    let toggle = if supports.iter().any(|s| matches!(s, Support::Toggle)) {
        protocol(model, &Support::Toggle)
    } else {
        vec![]
    };
    let effort = supports.iter().find_map(|s| match s {
        Support::Effort { values } => Some(Support::Effort { values: values.clone() }),
        _ => None,
    });
    let budget = supports.iter().find_map(|s| match s {
        Support::BudgetTokens { min, max } => Some(Support::BudgetTokens { min: *min, max: *max }),
        _ => None,
    });
    let main = if let Some(effort) = effort {
        protocol(model, &effort)
    } else if let Some(budget) = budget {
        protocol(model, &budget)
    } else {
        toggle.clone()
    };
    let mut variants: Vec<Variant> =
        toggle.into_iter().filter(|v| v.id.as_str() == "none").collect();
    variants.extend(main);
    let mut seen: Vec<String> = Vec::new();
    variants
        .into_iter()
        .filter(|v| {
            if seen.contains(&v.id.0) {
                false
            } else {
                seen.push(v.id.0.clone());
                true
            }
        })
        .collect()
}

impl Overlay {
    fn merge_body(mut self, body: BTreeMap<String, Value>) -> Self {
        let target = self.body.get_or_insert_with(BTreeMap::new);
        target.extend(body);
        self
    }
}

#[cfg(test)]
mod tests {
    // `assert!(x.is_empty())` reads better than the suggested assert_eq form.
    #![allow(clippy::assert_is_empty)]

    use super::*;

    fn model(package: &str, model_id: &str, output: i64) -> VariantModel {
        VariantModel {
            package: Some(package.to_string()),
            provider_id: "test".into(),
            model_id: model_id.into(),
            limit_output: output,
        }
    }

    #[test]
    fn provider_variant_anthropic_effort_adaptive() {
        let model = model("@opencode/ai/providers/anthropic", "claude-opus-5", 128_000);
        let variants = resolve(
            &model,
            &[Support::Effort { values: Some(vec!["low".into(), "medium".into(), "high".into()]) }],
        );
        let ids: Vec<&str> = variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, vec!["low", "medium", "high"]);
        let settings = variants[0].settings.as_ref().unwrap();
        assert_eq!(
            settings.rest.get("thinking"),
            Some(&json!({"type": "adaptive", "display": "summarized"}))
        );
        assert_eq!(settings.rest.get("effort"), Some(&json!("low")));
    }

    #[test]
    fn provider_variant_anthropic_budget_32k_clamp() {
        let model = model("@opencode/ai/providers/anthropic", "claude-haiku-4-5", 64_000);
        let variants = resolve(&model, &[Support::BudgetTokens { min: Some(1024), max: None }]);
        let ids: Vec<&str> = variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, vec!["high", "max"]);
        let high = variants[0].settings.as_ref().unwrap().rest.get("thinking").unwrap();
        assert_eq!(high["budgetTokens"], json!(16_000));
        let max = variants[1].settings.as_ref().unwrap().rest.get("thinking").unwrap();
        assert_eq!(max["budgetTokens"], json!(31_999), "clamped under the 32k ceiling");
    }

    #[test]
    fn provider_variant_anthropic_manual_models_fall_back_to_budget() {
        // claude-3-7 is "manual": effort redirects to budget_tokens min 1024
        let model = model("@opencode/ai/providers/anthropic", "claude-3-7-sonnet", 64_000);
        let variants = resolve(&model, &[Support::Effort { values: None }]);
        let ids: Vec<&str> = variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, vec!["high", "max"]);
    }

    #[test]
    fn provider_variant_openai_responses_encrypted_reasoning() {
        let model = model("@opencode/ai/providers/openai", "gpt-5.4", 32_000);
        let variants = resolve(&model, &[Support::Effort { values: None }]);
        let ids: Vec<&str> = variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, vec!["none", "minimal", "low", "medium", "high", "xhigh"]);
        let settings = variants[2].settings.as_ref().unwrap();
        assert_eq!(settings.rest.get("reasoningEffort"), Some(&json!("low")));
        assert_eq!(settings.rest.get("reasoningSummary"), Some(&json!("auto")));
        assert_eq!(settings.rest.get("include"), Some(&json!(["reasoning.encrypted_content"])));
    }

    #[test]
    fn provider_variant_alibaba_budget_64k_clamp() {
        let model = model("@opencode/ai/providers/alibaba/chat", "qwen3.5-flash", 65_536);
        let variants = resolve(
            &model,
            &[Support::Toggle, Support::BudgetTokens { min: None, max: Some(81_920) }],
        );
        // toggle contributes only "none"; budget wins over toggle for "main"
        let ids: Vec<&str> = variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, vec!["none", "high", "max"]);
        let high = variants[1].settings.as_ref().unwrap();
        assert_eq!(high.rest.get("thinkingBudget"), Some(&json!(32_000)));
        let max = variants[2].settings.as_ref().unwrap();
        assert_eq!(max.rest.get("thinkingBudget"), Some(&json!(63_999)));
        assert_eq!(max.rest.get("enableThinking"), Some(&json!(true)));
    }

    #[test]
    fn provider_variant_gemini_toggle() {
        let model = model("@opencode/ai/providers/google", "gemini-3-pro", 65_536);
        let variants = resolve(&model, &[Support::Toggle]);
        // toggle contributes "none"; with no effort/budget support the main
        // list IS the toggle list, so "thinking" survives dedupe (v2 parity)
        let ids: Vec<&str> = variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, vec!["none", "thinking"]);
        let none = variants[0].settings.as_ref().unwrap();
        assert_eq!(
            none.rest.get("thinkingConfig"),
            Some(&json!({"includeThoughts": false, "thinkingBudget": 0}))
        );
        let thinking = variants[1].settings.as_ref().unwrap();
        assert_eq!(
            thinking.rest.get("thinkingConfig"),
            Some(&json!({"includeThoughts": true, "thinkingBudget": -1}))
        );
    }

    #[test]
    fn provider_variant_unknown_package_yields_none() {
        let model = model("aisdk:@ai-sdk/unknown", "m1", 8_000);
        assert!(resolve(&model, &[Support::Effort { values: None }]).is_empty());
    }
}
