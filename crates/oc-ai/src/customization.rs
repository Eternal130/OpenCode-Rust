//! Per-provider customization seam (RC-06).
//!
//! The catalog collapses ~60 per-provider TS files into the
//! [`crate::protocols::openai_compatible`] wrapper plus a handful of
//! special cases (cache-policy quirks like Alibaba Qwen). Historically those
//! special cases lived as `if route == "..."` branches inside
//! [`crate::cache_policy`]; without a dedicated seam, every new per-provider
//! quirk degenerates into another scattered conditional.
//!
//! This module is that seam: a [`ProviderCustomization`] describes the
//! provider-specific overrides, and [`for_route`] looks the entry up by
//! provider/route id. Consulted from:
//!
//! - [`crate::cache_policy`] — the `auto_cache_policy` hook resolves the
//!   default policy for routes whose cache placement is model-dependent
//!   (OpenRouter/Vercel gateway aliases, `alibaba-chat` Qwen models).
//! - [`crate::protocols::openai_compatible`] — the `chat_metadata_key` hook
//!   overrides the re-badged `providerMetadataKey` when a provider needs a
//!   namespace other than `"openai"`.
//! - [`crate::protocols::anthropic_messages`] — the `wrap_anthropic_body`
//!   hook (RW-01) wraps the built body for the Anthropic-family providers
//!   with per-provider protocols in the ref: `alibaba`/`zai-coding-plan`
//!   re-attach `thinking` without Anthropic's mandatory budgetTokens, and
//!   `meta` remaps `native.meta` web-search tools.
//!
//! Extension point: to add a customization, either add a `const` entry to
//! [`static_entries`] (upstream providers) or call [`register`] (tests,
//! downstream crates). The unit test at the bottom of this file shows a
//! hypothetical provider registering one end to end.
//!
//! All hooks are `Option` and default to `None` (no-op), so a provider
//! without an entry behaves exactly as before this seam existed.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use serde_json::{Map, Value};

use crate::cache_policy::{auto_policy, none_policy};
use crate::protocols::shared::{fit_thinking_budget, invalid_request, number};
use crate::schema::{AiError, CachePolicyObject};

/// Provider-specific overrides. Every field defaults to `None` (= keep the
/// generic behavior); `Default` yields the no-op customization.
#[derive(Clone, Default, PartialEq, Debug)]
// `PartialEq` compares the `auto_cache_policy` fn pointer for test
// assertions; addresses of `fn` items are unique per monomorphization in
// practice, and the comparison is only ever between entries and `noop()`.
#[allow(unpredictable_function_pointer_comparisons)]
pub struct ProviderCustomization {
    /// Resolve the cache policy applied when the request carries no explicit
    /// policy (`cache: None | Auto`). Receives the model id; return
    /// `Some(policy)` to override the generic `auto` resolution.
    pub auto_cache_policy: Option<fn(&str) -> CachePolicyObject>,
    /// Override the `providerMetadataKey` re-badged onto
    /// `openai-compatible-chat` routes configured for this provider id.
    pub chat_metadata_key: Option<&'static str>,
    /// Wrap the request body built by the generic Anthropic Messages
    /// protocol (port of the per-provider `*-messages.ts` body wrappers).
    ///
    /// When set, the generic body build receives the request with the raw
    /// `thinking` provider option stripped (so the generic
    /// "`enabled` requires a budgetTokens" validation and budget fitting
    /// do not run), and the hook re-attaches the provider's own `thinking`
    /// shape and/or remaps tools afterwards.
    pub wrap_anthropic_body: Option<fn(&mut AnthropicBodyWrap<'_>) -> Result<(), AiError>>,
}

/// Context handed to the [`ProviderCustomization::wrap_anthropic_body`] hook.
pub struct AnthropicBodyWrap<'a> {
    /// Provider id the customization was looked up by.
    pub provider: &'a str,
    /// The raw `thinking` provider-option value stripped before the generic
    /// body build (`None` when the request carried no `thinking` option).
    pub raw_thinking: Option<&'a Value>,
    /// Flattened tool `native` maps, index-aligned with the built body's
    /// `tools` array entries (empty when the body has no tools).
    pub tool_natives: &'a [Option<&'a BTreeMap<String, Value>>],
    /// The body's `max_tokens` (default applied).
    pub max_tokens: f64,
    /// The built Anthropic Messages body; the hook mutates it in place.
    pub body: &'a mut Value,
}

impl AnthropicBodyWrap<'_> {
    /// Set `body.thinking` to `value`, or remove the key when `None`
    /// (the ref spreads `thinking: undefined`, dropping the key).
    pub fn set_thinking(&mut self, value: Option<Value>) {
        let Some(object) = self.body.as_object_mut() else { return };
        match value {
            Some(value) => {
                object.insert("thinking".into(), value);
            }
            None => {
                object.remove("thinking");
            }
        }
    }
}

impl ProviderCustomization {
    /// The no-op customization: every hook unset, generic behavior everywhere.
    pub const fn noop() -> Self {
        Self { auto_cache_policy: None, chat_metadata_key: None, wrap_anthropic_body: None }
    }

    /// Apply the `auto_cache_policy` hook if the entry sets one.
    #[must_use]
    pub fn resolve_auto_cache_policy(&self, model_id: &str) -> Option<CachePolicyObject> {
        self.auto_cache_policy.map(|resolve| resolve(model_id))
    }
}

/// Gemini uses only the last breakpoint, so a conversation-tail breakpoint
/// writes a new cache every step and costs more than none. Qwen ignores
/// breakpoints on tool definitions and caches tools with the system prompt.
fn qwen_policy() -> CachePolicyObject {
    CachePolicyObject {
        tools: None,
        system: Some(true),
        messages: Some(crate::schema::CachePolicyMessages::Tail { tail: 1 }),
        ttl_seconds: None,
    }
}

/// Gateway routes (`OpenRouter`, Vercel AI Gateway) follow the upstream model
/// alias: `~anthropic/...` keeps the full auto policy, `qwen/...`-style ids
/// get the Qwen policy, everything else caches implicitly (no breakpoints).
fn gateway_policy(model_id: &str) -> CachePolicyObject {
    // `~anthropic/claude-sonnet-latest` style IDs are OpenRouter aliases.
    let id = model_id.trim_start_matches('~');
    if id.starts_with("anthropic/") {
        auto_policy()
    } else if id.starts_with("qwen/") || id.starts_with("alibaba/qwen") {
        qwen_policy()
    } else {
        none_policy()
    }
}

/// `alibaba-chat`: Qwen models cache with the system prompt + tail; the rest
/// of the catalog (deepseek et al.) caches implicitly.
fn alibaba_chat_policy(model_id: &str) -> CachePolicyObject {
    if model_id.to_lowercase().starts_with("qwen") {
        qwen_policy()
    } else {
        none_policy()
    }
}

/// `alibaba-messages` body wrap (port of `alibaba-messages.ts:29-46`): Model
/// Studio accepts enabled thinking without Anthropic's mandatory token
/// budget. The generic build ran with `thinking` stripped; re-attach it from
/// the raw option, fitting an explicit budget (`budgetTokens` or
/// `budget_tokens`) against `max_tokens` like `ProviderShared.fitThinkingBudget`.
fn alibaba_wrap(wrap: &mut AnthropicBodyWrap<'_>) -> Result<(), AiError> {
    let Some(raw) = wrap.raw_thinking else {
        wrap.set_thinking(None);
        return Ok(());
    };
    let Some(object) = raw.as_object() else {
        return Err(invalid_request(format!("Expected an object, received {raw}")));
    };
    let thinking_type = want_tool_string(object.get("type"))?;
    // `opts.thinking?.budgetTokens ?? opts.thinking?.budget_tokens`
    let budget = match (object.get("budgetTokens"), object.get("budget_tokens")) {
        (Some(value), _) | (None, Some(value)) => Some(want_integer(value)?),
        (None, None) => None,
    };
    let mut thinking = Map::new();
    thinking.insert("type".into(), serde_json::json!(thinking_type));
    if let Some(budget) = budget {
        let fitted = fit_thinking_budget(budget, Some(wrap.max_tokens), 1.0);
        thinking.insert("budget_tokens".into(), number(fitted));
    }
    wrap.set_thinking(Some(Value::Object(thinking)));
    Ok(())
}

/// `zai-messages` body wrap (port of `zai-messages.ts` `fromRequest`): Z.AI
/// accepts enabled thinking without Anthropic's mandatory token budget; the
/// wire shape is `{ type }` only.
fn zai_wrap(wrap: &mut AnthropicBodyWrap<'_>) -> Result<(), AiError> {
    let Some(raw) = wrap.raw_thinking else {
        wrap.set_thinking(None);
        return Ok(());
    };
    let Some(object) = raw.as_object() else {
        return Err(invalid_request(format!("Expected an object, received {raw}")));
    };
    let thinking_type = want_tool_string(object.get("type"))?;
    wrap.set_thinking(Some(serde_json::json!({"type": thinking_type})));
    Ok(())
}

/// `meta-messages` body wrap (port of `meta-messages.ts` `fromRequest`):
/// flatten-tool natives carrying `native.meta` web-search config are re-mapped
/// to Anthropic's `{ type: "web_search", name: "web_search", user_location }`.
fn meta_wrap(wrap: &mut AnthropicBodyWrap<'_>) -> Result<(), AiError> {
    let Some(tools) = wrap.body.get_mut("tools").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for (index, tool) in tools.iter_mut().enumerate() {
        let Some(native) = wrap.tool_natives.get(index).copied().flatten() else {
            continue;
        };
        // Ref decodes `native.meta` with the WebSearch schema — a missing
        // `meta` is an invalid request, not a silently-kept generic tool
        // (meta-messages.ts:38-42).
        let meta = native
            .get("meta")
            .ok_or_else(|| invalid_request("Expected an object, received undefined"))?;
        let Some(search) = meta.as_object() else {
            return Err(invalid_request(format!("Expected an object, received {meta}")));
        };
        if search.get("type").and_then(Value::as_str) != Some("web_search") {
            let found = search.get("type").map_or_else(|| "undefined".into(), Value::to_string);
            return Err(invalid_request(format!("Expected \"web_search\", received {found}")));
        }
        if let Some(size) = search.get("search_context_size") {
            if size.as_str().is_none() {
                return Err(invalid_request(format!("Expected a string, received {size}")));
            }
            return Err(invalid_request("Meta Messages does not support searchContextSize"));
        }
        let user_location = match search.get("user_location") {
            None => None,
            Some(location) => {
                let Some(location_object) = location.as_object() else {
                    return Err(invalid_request(format!(
                        "Expected an object, received {location}"
                    )));
                };
                if location_object.get("type").and_then(Value::as_str) != Some("approximate") {
                    let found = location_object
                        .get("type")
                        .map_or_else(|| "undefined".into(), Value::to_string);
                    return Err(invalid_request(format!(
                        "Expected \"approximate\", received {found}"
                    )));
                }
                for key in ["city", "region", "country", "timezone"] {
                    if let Some(value) = location_object.get(key) {
                        if value.as_str().is_none() {
                            return Err(invalid_request(format!(
                                "Expected a string, received {value}"
                            )));
                        }
                    }
                }
                // The ref schema emits only the declared keys
                // (meta-responses.ts:15-23), not the whole input object.
                let mut location = Map::new();
                for key in ["type", "city", "region", "country", "timezone"] {
                    if let Some(value) = location_object.get(key) {
                        location.insert(key.into(), value.clone());
                    }
                }
                Some(Value::Object(location))
            }
        };
        let mut remapped = Map::new();
        remapped.insert("type".into(), serde_json::json!("web_search"));
        remapped.insert("name".into(), serde_json::json!("web_search"));
        if let Some(user_location) = user_location {
            remapped.insert("user_location".into(), user_location);
        }
        *tool = Value::Object(remapped);
    }
    Ok(())
}

/// `Schema.optional(Schema.String)`-style field check for the wrap schemas.
fn want_tool_string(value: Option<&Value>) -> Result<String, AiError> {
    match value {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(other) => Err(invalid_request(format!("Expected a string, received {other}"))),
        None => Err(invalid_request("Expected a string, received undefined")),
    }
}

fn want_number(value: &Value) -> Result<f64, AiError> {
    value.as_f64().ok_or_else(|| invalid_request(format!("Expected a number, received {value}")))
}

/// `Schema.Int` check (ref alibaba-messages.ts:13-14): the thinking budget
/// must be a number without a fractional part.
fn want_integer(value: &Value) -> Result<f64, AiError> {
    let number = want_number(value)?;
    if number.fract() != 0.0 {
        return Err(invalid_request(format!("Expected an integer, received {value}")));
    }
    Ok(number)
}

/// Static registry entries for the providers with known special cases.
/// New upstream special cases go here (see module docs).
const fn static_entries() -> &'static [(&'static str, ProviderCustomization)] {
    &[
        (
            "openrouter",
            ProviderCustomization {
                auto_cache_policy: Some(gateway_policy),
                chat_metadata_key: None,
                wrap_anthropic_body: None,
            },
        ),
        (
            "vercel-ai-gateway-messages",
            ProviderCustomization {
                auto_cache_policy: Some(gateway_policy),
                chat_metadata_key: None,
                wrap_anthropic_body: None,
            },
        ),
        (
            "alibaba-chat",
            ProviderCustomization {
                auto_cache_policy: Some(alibaba_chat_policy),
                chat_metadata_key: None,
                wrap_anthropic_body: None,
            },
        ),
        (
            "alibaba",
            ProviderCustomization {
                auto_cache_policy: None,
                chat_metadata_key: None,
                wrap_anthropic_body: Some(alibaba_wrap),
            },
        ),
        (
            "zai-coding-plan",
            ProviderCustomization {
                auto_cache_policy: None,
                chat_metadata_key: None,
                wrap_anthropic_body: Some(zai_wrap),
            },
        ),
        (
            "meta",
            ProviderCustomization {
                auto_cache_policy: None,
                chat_metadata_key: None,
                wrap_anthropic_body: Some(meta_wrap),
            },
        ),
    ]
}

/// Runtime registrations (tests / downstream crates). Keyed like
/// [`static_entries`]: by provider or route id.
fn registered() -> &'static Mutex<Vec<(String, ProviderCustomization)>> {
    static REGISTERED: OnceLock<Mutex<Vec<(String, ProviderCustomization)>>> = OnceLock::new();
    REGISTERED.get_or_init(|| Mutex::new(Vec::new()))
}

/// Register a customization for a provider id. Later registrations for the
/// same id replace earlier ones; runtime entries take precedence over the
/// [`static_entries`]. Intended for tests and downstream crates — upstream
/// special cases belong in [`static_entries`].
pub fn register(provider: &str, customization: ProviderCustomization) {
    let mut entries = registered().lock().expect("customization registry poisoned");
    entries.retain(|(id, _)| id != provider);
    entries.push((provider.to_string(), customization));
}

/// Look up the customization for a provider/route id. Unregistered ids
/// resolve to the no-op customization (generic behavior everywhere).
///
/// The `id` is a provider id or a route id depending on which consumer
/// hook performs the lookup (e.g. the cache policy resolution passes the
/// resolved route), so entries must register under whichever key the
/// relevant consumer will query.
#[must_use]
pub fn for_route(id: &str) -> ProviderCustomization {
    if let Some((_, customization)) = static_entries().iter().find(|(key, _)| *key == id) {
        return customization.clone();
    }
    let entries = registered().lock().expect("customization registry poisoned");
    entries
        .iter()
        .find(|(key, _)| key == id)
        .map_or_else(ProviderCustomization::noop, |(_, customization)| customization.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extension-point documentation: a hypothetical provider registers a
    /// customization and both consultation paths (cache policy + the
    /// openai-compatible metadata key) pick it up, while providers without
    /// entries keep the no-op defaults.
    #[test]
    fn hypothetical_provider_registers_a_customization() {
        fn hypothetical_policy(model_id: &str) -> CachePolicyObject {
            if model_id.starts_with("cached-") {
                auto_policy()
            } else {
                none_policy()
            }
        }

        register(
            "acme",
            ProviderCustomization {
                auto_cache_policy: Some(hypothetical_policy),
                chat_metadata_key: Some("acme"),
                wrap_anthropic_body: None,
            },
        );

        let customization = for_route("acme");
        assert_eq!(
            customization.resolve_auto_cache_policy("cached-x"),
            Some(auto_policy()),
            "hook delegates to the registered policy resolver"
        );
        assert_eq!(customization.resolve_auto_cache_policy("other"), Some(none_policy()));
        assert_eq!(customization.chat_metadata_key, Some("acme"));

        // unregistered providers keep the no-op defaults
        assert_eq!(
            for_route("not-a-provider"),
            ProviderCustomization::noop(),
            "missing entries resolve to the no-op customization"
        );

        // static entries remain reachable and unchanged
        assert_eq!(
            for_route("openrouter").resolve_auto_cache_policy("qwen/qwen3-coder"),
            Some(qwen_policy())
        );
        assert_eq!(
            for_route("alibaba-chat").resolve_auto_cache_policy("Qwen3-Max"),
            Some(qwen_policy())
        );
        assert_eq!(
            for_route("alibaba-chat").resolve_auto_cache_policy("deepseek-v3"),
            Some(none_policy())
        );
    }
}

#[cfg(test)]
mod rw01_tests {
    use super::*;

    /// RW-01: the wrap_anthropic_body hook reaches the Anthropic-family
    /// providers with body wrappers, and everyone else keeps the no-op.
    #[test]
    fn anthropic_body_wrap_registry_entries() {
        assert!(for_route("alibaba").wrap_anthropic_body.is_some());
        assert!(for_route("zai-coding-plan").wrap_anthropic_body.is_some());
        assert!(for_route("meta").wrap_anthropic_body.is_some());
        // unregistered providers (incl. ref's plain-protocol moonshot/minimax
        // messages routes) and the pre-existing entries stay unchanged
        assert_eq!(for_route("moonshotai"), ProviderCustomization::noop());
        assert_eq!(for_route("minimax"), ProviderCustomization::noop());
        assert_eq!(for_route("anthropic"), ProviderCustomization::noop());
        assert_eq!(for_route("openrouter").wrap_anthropic_body, None);
        assert_eq!(for_route("alibaba-chat").wrap_anthropic_body, None);
        // runtime registration can still set the hook (extension point)
        fn wrap(_: &mut AnthropicBodyWrap<'_>) -> Result<(), crate::schema::AiError> {
            Ok(())
        }
        register(
            "acme-anthropic",
            ProviderCustomization {
                wrap_anthropic_body: Some(wrap),
                ..ProviderCustomization::noop()
            },
        );
        assert!(for_route("acme-anthropic").wrap_anthropic_body.is_some());
    }
}

#[cfg(test)]
mod rw2fix_tests {
    use super::*;

    fn wrap_context<'a>(
        body: &'a mut Value,
        natives: &'static [Option<&'static BTreeMap<String, Value>>],
    ) -> AnthropicBodyWrap<'a> {
        AnthropicBodyWrap {
            provider: "meta",
            raw_thinking: None,
            tool_natives: natives,
            max_tokens: 1024.0,
            body,
        }
    }

    fn static_native(value: Value) -> &'static BTreeMap<String, Value> {
        Box::leak(serde_json::from_value(value).expect("native map"))
    }

    /// RW2 fix: a flatten-tool `native` without `meta` is an invalid
    /// request (ref decodes `native.meta` with the WebSearch schema —
    /// meta-messages.ts:38-42), not a silently-kept generic tool.
    #[test]
    fn meta_wrap_errors_on_missing_native_meta() {
        let mut body = serde_json::json!({"tools": [{"type": "custom_tool", "name": "t"}]});
        let empty: &'static BTreeMap<String, Value> = Box::leak(Box::new(BTreeMap::new()));
        let natives: &'static [Option<&'static BTreeMap<String, Value>>] =
            Box::leak(vec![Some(empty)].into_boxed_slice());
        let mut context = wrap_context(&mut body, natives);
        let error = meta_wrap(&mut context).expect_err("missing native.meta rejected");
        assert!(
            error.message().contains("Expected an object"),
            "unexpected message: {}",
            error.message()
        );
    }

    /// RW2 fix: `user_location` re-emits only the schema keys
    /// (meta-responses.ts:15-23) — extra input keys are dropped.
    #[test]
    fn meta_wrap_user_location_keeps_only_schema_keys() {
        let mut body = serde_json::json!({"tools": [{"type": "custom_tool", "name": "t"}]});
        let native = static_native(serde_json::json!({
            "meta": {
                "type": "web_search",
                "user_location": {
                    "type": "approximate",
                    "city": "Springfield",
                    "extra": "dropped",
                },
            },
        }));
        let natives: &'static [Option<&'static BTreeMap<String, Value>>] =
            Box::leak(vec![Some(native)].into_boxed_slice());
        let mut context = wrap_context(&mut body, natives);
        meta_wrap(&mut context).expect("wrap ok");
        let location = &body["tools"][0]["user_location"];
        assert_eq!(location["type"], "approximate");
        assert_eq!(location["city"], "Springfield");
        assert!(location.get("extra").is_none(), "extra keys dropped: {location}");
    }

    /// RW2 fix: the alibaba thinking budget is `Schema.Int` — a
    /// fractional value is rejected (alibaba-messages.ts:13-14).
    #[test]
    fn alibaba_wrap_rejects_fractional_thinking_budget() {
        let raw = serde_json::json!({"type": "enabled", "budgetTokens": 1024.5});
        let mut body = serde_json::json!({"max_tokens": 4096});
        let mut context = AnthropicBodyWrap {
            provider: "alibaba",
            raw_thinking: Some(&raw),
            tool_natives: &[],
            max_tokens: 4096.0,
            body: &mut body,
        };
        let error = alibaba_wrap(&mut context).expect_err("fractional budget rejected");
        assert!(error.message().contains("integer"), "unexpected message: {}", error.message());
        // integers keep working
        let raw = serde_json::json!({"type": "enabled", "budgetTokens": 1024});
        let mut body = serde_json::json!({"max_tokens": 4096});
        let mut context = AnthropicBodyWrap {
            provider: "alibaba",
            raw_thinking: Some(&raw),
            tool_natives: &[],
            max_tokens: 4096.0,
            body: &mut body,
        };
        alibaba_wrap(&mut context).expect("integer budget accepted");
        assert_eq!(body["thinking"]["budget_tokens"], 1024.0);
    }
}
