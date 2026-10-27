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
//!
//! Extension point: to add a customization, either add a `const` entry to
//! [`static_entries`] (upstream providers) or call [`register`] (tests,
//! downstream crates). The unit test at the bottom of this file shows a
//! hypothetical provider registering one end to end.
//!
//! All hooks are `Option` and default to `None` (no-op), so a provider
//! without an entry behaves exactly as before this seam existed.

use std::sync::{Mutex, OnceLock};

use crate::cache_policy::{auto_policy, none_policy};
use crate::schema::CachePolicyObject;

/// Provider-specific overrides. Every field defaults to `None` (= keep the
/// generic behavior); `Default` yields the no-op customization.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct ProviderCustomization {
    /// Resolve the cache policy applied when the request carries no explicit
    /// policy (`cache: None | Auto`). Receives the model id; return
    /// `Some(policy)` to override the generic `auto` resolution.
    pub auto_cache_policy: Option<fn(&str) -> CachePolicyObject>,
    /// Override the `providerMetadataKey` re-badged onto
    /// `openai-compatible-chat` routes configured for this provider id.
    pub chat_metadata_key: Option<&'static str>,
}

impl ProviderCustomization {
    /// The no-op customization: every hook unset, generic behavior everywhere.
    pub const fn noop() -> Self {
        Self { auto_cache_policy: None, chat_metadata_key: None }
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

/// Gateway routes (OpenRouter, Vercel AI Gateway) follow the upstream model
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

/// Static registry entries for the providers with known special cases.
/// New upstream special cases go here (see module docs).
const fn static_entries() -> &'static [(&'static str, ProviderCustomization)] {
    &[
        (
            "openrouter",
            ProviderCustomization {
                auto_cache_policy: Some(gateway_policy),
                chat_metadata_key: None,
            },
        ),
        (
            "vercel-ai-gateway-messages",
            ProviderCustomization {
                auto_cache_policy: Some(gateway_policy),
                chat_metadata_key: None,
            },
        ),
        (
            "alibaba-chat",
            ProviderCustomization {
                auto_cache_policy: Some(alibaba_chat_policy),
                chat_metadata_key: None,
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
