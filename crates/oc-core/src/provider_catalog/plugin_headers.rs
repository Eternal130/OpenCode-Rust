//! Static provider header-injection plugins (AUD-10②).
//!
//! Port of the four ref plugins that key on the resolved package +
//! `settings.baseURL` and merge a static header set into
//! `provider.headers`:
//!
//! - `cerebras` (ref `plugin/provider/cerebras.ts:9-12`) —
//!   `X-Cerebras-3rd-Party-Integration: opencode` for every cerebras
//!   package (no baseURL/activation gate);
//! - `kilo` (ref `plugin/provider/kilo.ts:9-13`) — openai-compatible
//!   package at exactly `https://api.kilo.ai/api/gateway` gets
//!   `HTTP-Referer: https://opencode.ai/` + `X-Title: opencode`;
//! - `nvidia` (ref `plugin/provider/nvidia.ts:9-17`) — openai-compatible
//!   at `https://integrate.api.nvidia.com/v1` gets the kilo pair plus
//!   `X-BILLING-INVOKE-ORIGIN` keeping any user-set value
//!   (`provider.headers?.[...] ?? "OpenCode"`);
//! - `llmgateway` (ref `plugin/provider/llmgateway.ts:12-22`) —
//!   openai-compatible at `https://api.llmgateway.io/v1`, activation not
//!   disabled AND an integration registered under the provider id, gets
//!   `HTTP-Referer`/`X-Title` plus `X-Source: opencode`.
//!
//! All four spread over the provider's existing headers
//! (`provider.headers = { ...provider.headers, ... }`), so the injected
//! values win per key; a `None`/non-matching `base_url` injects nothing
//! (the ref `!==` comparisons fail on `undefined`).

use std::collections::BTreeMap;

use oc_schema::provider::Activation;

const OPENAI_COMPATIBLE: &str = "@opencode/ai/providers/openai-compatible";
const CEREBRAS: &str = "@opencode/ai/providers/cerebras";

const REFERER: (&str, &str) = ("HTTP-Referer", "https://opencode.ai/");
const TITLE: (&str, &str) = ("X-Title", "opencode");

/// Extra headers the ref provider plugins inject for this provider, or
/// an empty map when no plugin matches.
///
/// `existing` is the provider's user-configured header set (the ref
/// spread base — only nvidia consults it, for `X-BILLING-INVOKE-ORIGIN`).
/// `integration_registered` mirrors the llmgateway plugin's
/// `configured.has(Integration.ID.make(provider.id))` gate (the caller
/// resolves registration for the bound provider id).
#[must_use]
pub fn plugin_provider_headers(
    package: &str,
    base_url: Option<&str>,
    activation: Activation,
    integration_registered: bool,
    existing: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    let base = base_url.filter(|_| package == OPENAI_COMPATIBLE);
    match package {
        // cerebras.ts:9-12 — package match only.
        CEREBRAS => {
            headers.insert("X-Cerebras-3rd-Party-Integration".to_string(), "opencode".to_string());
        }
        // kilo.ts:10 — exact settings.baseURL equality.
        _ if base == Some("https://api.kilo.ai/api/gateway") => {
            headers.insert(REFERER.0.to_string(), REFERER.1.to_string());
            headers.insert(TITLE.0.to_string(), TITLE.1.to_string());
        }
        // nvidia.ts:10-17 — exact baseURL; keeps a user-set billing origin.
        _ if base == Some("https://integrate.api.nvidia.com/v1") => {
            headers.insert(REFERER.0.to_string(), REFERER.1.to_string());
            headers.insert(TITLE.0.to_string(), TITLE.1.to_string());
            let origin = existing
                .get("X-BILLING-INVOKE-ORIGIN")
                .cloned()
                .unwrap_or_else(|| "OpenCode".to_string());
            headers.insert("X-BILLING-INVOKE-ORIGIN".to_string(), origin);
        }
        // llmgateway.ts:12-15 — active provider, exact baseURL, registered
        // integration under the provider id.
        _ if base == Some("https://api.llmgateway.io/v1")
            && activation != Activation::Disabled
            && integration_registered =>
        {
            headers.insert(REFERER.0.to_string(), REFERER.1.to_string());
            headers.insert(TITLE.0.to_string(), TITLE.1.to_string());
            headers.insert("X-Source".to_string(), "opencode".to_string());
        }
        _ => {}
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    #[test]
    fn cerebras_package_gets_integration_header_without_baseurl_gate() {
        // cerebras.ts:9-12 keys on the package only — a missing or odd
        // baseURL still injects.
        for base in [None, Some("https://example.test/v1")] {
            let headers = plugin_provider_headers(
                "@opencode/ai/providers/cerebras",
                base,
                Activation::Disabled,
                false,
                &existing(),
            );
            assert_eq!(
                headers,
                BTreeMap::from([(
                    "X-Cerebras-3rd-Party-Integration".to_string(),
                    "opencode".to_string()
                )]),
                "base {base:?}"
            );
        }
    }

    #[test]
    fn kilo_exact_baseurl_gets_referer_and_title() {
        let headers = plugin_provider_headers(
            OPENAI_COMPATIBLE,
            Some("https://api.kilo.ai/api/gateway"),
            Activation::Auto,
            false,
            &existing(),
        );
        assert_eq!(
            headers,
            BTreeMap::from([
                ("HTTP-Referer".to_string(), "https://opencode.ai/".to_string()),
                ("X-Title".to_string(), "opencode".to_string()),
            ])
        );
    }

    #[test]
    fn kilo_non_matching_baseurl_injects_nothing() {
        // kilo.ts:10 `!==` — trailing slash, prefix, or None all fail.
        for base in [
            None,
            Some("https://api.kilo.ai/api/gateway/"),
            Some("https://api.kilo.ai/api/gateway/v1"),
            Some("https://other.test/api/gateway"),
        ] {
            assert!(
                plugin_provider_headers(
                    OPENAI_COMPATIBLE,
                    base,
                    Activation::Enabled,
                    false,
                    &existing()
                )
                .is_empty(),
                "base {base:?} must not inject"
            );
        }
    }

    #[test]
    fn kilo_requires_the_openai_compatible_package() {
        assert!(
            plugin_provider_headers(
                "@opencode/ai/providers/openai/chat",
                Some("https://api.kilo.ai/api/gateway"),
                Activation::Enabled,
                false,
                &existing(),
            )
            .is_empty()
        );
    }

    #[test]
    fn nvidia_defaults_billing_origin_and_keeps_user_value() {
        let headers = plugin_provider_headers(
            OPENAI_COMPATIBLE,
            Some("https://integrate.api.nvidia.com/v1"),
            Activation::Auto,
            false,
            &existing(),
        );
        assert_eq!(
            headers,
            BTreeMap::from([
                ("HTTP-Referer".to_string(), "https://opencode.ai/".to_string()),
                ("X-Title".to_string(), "opencode".to_string()),
                ("X-BILLING-INVOKE-ORIGIN".to_string(), "OpenCode".to_string()),
            ])
        );
        // nvidia.ts:16 — an existing provider header wins over the default.
        let headers = plugin_provider_headers(
            OPENAI_COMPATIBLE,
            Some("https://integrate.api.nvidia.com/v1"),
            Activation::Auto,
            false,
            &BTreeMap::from([(
                "X-BILLING-INVOKE-ORIGIN".to_string(),
                "custom-origin".to_string()
            )]),
        );
        assert_eq!(headers.get("X-BILLING-INVOKE-ORIGIN").map(String::as_str), Some("custom-origin"));
    }

    #[test]
    fn llmgateway_requires_baseurl_activation_and_registered_integration() {
        let headers = plugin_provider_headers(
            OPENAI_COMPATIBLE,
            Some("https://api.llmgateway.io/v1"),
            Activation::Auto,
            true,
            &existing(),
        );
        assert_eq!(
            headers,
            BTreeMap::from([
                ("HTTP-Referer".to_string(), "https://opencode.ai/".to_string()),
                ("X-Title".to_string(), "opencode".to_string()),
                ("X-Source".to_string(), "opencode".to_string()),
            ])
        );
        // llmgateway.ts:12 — disabled providers are skipped; :15 — no
        // registered integration under the provider id is skipped.
        for (activation, registered) in
            [(Activation::Disabled, true), (Activation::Auto, false)]
        {
            assert!(
                plugin_provider_headers(
                    OPENAI_COMPATIBLE,
                    Some("https://api.llmgateway.io/v1"),
                    activation,
                    registered,
                    &existing(),
                )
                .is_empty(),
                "activation {activation:?} registered {registered} must not inject"
            );
        }
    }

    #[test]
    fn unrelated_providers_get_no_headers() {
        for package in [
            "@opencode/ai/providers/anthropic",
            "@opencode/ai/providers/openai/chat",
            "@opencode/ai/providers/openai-compatible",
        ] {
            assert!(
                plugin_provider_headers(
                    package,
                    Some("https://api.openai.com/v1"),
                    Activation::Enabled,
                    true,
                    &existing(),
                )
                .is_empty(),
                "package {package} must not inject"
            );
        }
    }
}
