//! Static provider header-injection plugins (AUD-10② + G-261①/G-285③).
//!
//! Port of the ref plugins that key on the resolved package +
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
//!   `HTTP-Referer`/`X-Title` plus `X-Source: opencode`;
//! - `openrouter` (G-261①, ref `plugin/provider/openrouter.ts:8-15`) —
//!   every provider bound to the openrouter package gets
//!   `HTTP-Referer`/`X-Title` (no baseURL/activation gate); injected
//!   values win (`{...provider.headers, ...}` spread);
//! - `zenmux` (G-285③, ref `plugin/provider/zenmux.ts:8-19`) —
//!   openai-compatible at exactly `https://zenmux.ai/api/v1` gets the
//!   same pair, but spread FIRST (`{HTTP-Referer, X-Title,
//!   ...provider.headers}`) so USER headers win — the opposite
//!   precedence of every other plugin here;
//! - `vercel` (G-285③, ref `plugin/provider/vercel.ts:8-16`) — the
//!   aisdk `@ai-sdk/vercel` package gets the LOWERCASE pair
//!   `http-referer`/`x-title` (injected values win). Bounded deviation:
//!   the ref gates on `Provider.isAISDK(package)` (the runtime
//!   npm-install storage form); this port sees the unwrapped package
//!   name, and no natively bound provider carries the bare
//!   `@ai-sdk/vercel` name (`family_of` has no entry, binding fails
//!   before headers), so keying on the name cannot false-positive.
//!
//! Everything except zenmux spreads over the provider's existing headers
//! (`provider.headers = { ...provider.headers, ... }`), so the injected
//! values win per key; a `None`/non-matching `base_url` injects nothing
//! (the ref `!==` comparisons fail on `undefined`).

use std::collections::BTreeMap;

use oc_schema::provider::Activation;

const OPENAI_COMPATIBLE: &str = "@opencode/ai/providers/openai-compatible";
const CEREBRAS: &str = "@opencode/ai/providers/cerebras";
const OPENROUTER: &str = "@opencode/ai/providers/openrouter";
const VERCEL: &str = "@ai-sdk/vercel";

const REFERER: (&str, &str) = ("HTTP-Referer", "https://opencode.ai/");
const TITLE: (&str, &str) = ("X-Title", "opencode");

/// The static header set a ref provider plugin injects, plus its spread
/// precedence over the provider's user-configured headers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginHeaderInjection {
    /// The injected static set (empty when no plugin matches).
    pub headers: BTreeMap<String, String>,
    /// Whether USER headers win over the injected set. Every plugin but
    /// zenmux spreads the injected values LAST
    /// (`{...provider.headers, ...injected}`); zenmux spreads them FIRST
    /// (`{...injected, ...provider.headers}`, zenmux.ts:12-16).
    pub user_headers_win: bool,
}

impl PluginHeaderInjection {
    /// Merge the injection over `existing` honoring the plugin's spread
    /// order: by default the injected values replace per key; with
    /// [`Self::user_headers_win`] the injected values only fill keys the
    /// user did not set.
    #[must_use]
    pub fn merged_over(self, existing: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        let mut out = existing.clone();
        if self.user_headers_win {
            for (name, value) in self.headers {
                out.entry(name).or_insert(value);
            }
        } else {
            out.extend(self.headers);
        }
        out
    }
}

/// The headers the ref provider plugins inject for this provider, or an
/// empty set when no plugin matches.
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
) -> PluginHeaderInjection {
    let mut injection = PluginHeaderInjection::default();
    let headers = &mut injection.headers;
    let base = base_url.filter(|_| package == OPENAI_COMPATIBLE);
    match package {
        // cerebras.ts:9-12 — package match only.
        CEREBRAS => {
            headers.insert("X-Cerebras-3rd-Party-Integration".to_string(), "opencode".to_string());
        }
        // openrouter.ts:9-14 — package match only; the injected pair
        // wins per key.
        OPENROUTER => {
            headers.insert(REFERER.0.to_string(), REFERER.1.to_string());
            headers.insert(TITLE.0.to_string(), TITLE.1.to_string());
        }
        // vercel.ts:12-14 — the aisdk @ai-sdk/vercel package; the
        // LOWERCASE pair (spread last, injected wins).
        VERCEL => {
            headers.insert("http-referer".to_string(), "https://opencode.ai/".to_string());
            headers.insert("x-title".to_string(), "opencode".to_string());
        }
        // kilo.ts:10 — exact settings.baseURL equality.
        _ if base == Some("https://api.kilo.ai/api/gateway") => {
            headers.insert(REFERER.0.to_string(), REFERER.1.to_string());
            headers.insert(TITLE.0.to_string(), TITLE.1.to_string());
        }
        // zenmux.ts:10-17 — exact baseURL; the injected pair spreads
        // FIRST, so user headers win.
        _ if base == Some("https://zenmux.ai/api/v1") => {
            injection.user_headers_win = true;
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
    injection
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    fn pair(lowercase: bool) -> BTreeMap<String, String> {
        if lowercase {
            BTreeMap::from([
                ("http-referer".to_string(), "https://opencode.ai/".to_string()),
                ("x-title".to_string(), "opencode".to_string()),
            ])
        } else {
            BTreeMap::from([
                ("HTTP-Referer".to_string(), "https://opencode.ai/".to_string()),
                ("X-Title".to_string(), "opencode".to_string()),
            ])
        }
    }

    #[test]
    fn cerebras_package_gets_integration_header_without_baseurl_gate() {
        // cerebras.ts:9-12 keys on the package only — a missing or odd
        // baseURL still injects.
        for base in [None, Some("https://example.test/v1")] {
            let injection = plugin_provider_headers(
                "@opencode/ai/providers/cerebras",
                base,
                Activation::Disabled,
                false,
                &existing(),
            );
            assert_eq!(
                injection.headers,
                BTreeMap::from([(
                    "X-Cerebras-3rd-Party-Integration".to_string(),
                    "opencode".to_string()
                )]),
                "base {base:?}"
            );
            assert!(!injection.user_headers_win);
        }
    }

    /// G-261① — openrouter keys on the package alone (openrouter.ts:9-14);
    /// the injected pair wins over user headers like the four AUD-10②
    /// plugins.
    #[test]
    fn openrouter_package_gets_referer_and_title_injected_over_user_headers() {
        for base in [None, Some("https://openrouter.ai/api/v1")] {
            let injection = plugin_provider_headers(
                OPENROUTER,
                base,
                Activation::Disabled,
                false,
                &BTreeMap::from([(
                    "HTTP-Referer".to_string(),
                    "https://custom.example/".to_string(),
                )]),
            );
            assert_eq!(injection.headers, pair(false), "base {base:?}");
            assert!(!injection.user_headers_win);
            let merged = injection.merged_over(&BTreeMap::from([(
                "HTTP-Referer".to_string(),
                "https://custom.example/".to_string(),
            )]));
            assert_eq!(
                merged.get("HTTP-Referer").map(String::as_str),
                Some("https://opencode.ai/"),
                "injected values win per key"
            );
        }
    }

    /// G-285③ — zenmux keys on openai-compatible + the exact baseURL
    /// (zenmux.ts:9-10); the injected pair spreads FIRST, so a user-set
    /// header survives.
    #[test]
    fn zenmux_exact_baseurl_injects_under_user_headers() {
        let user =
            BTreeMap::from([("HTTP-Referer".to_string(), "https://mine.example/".to_string())]);
        let injection = plugin_provider_headers(
            OPENAI_COMPATIBLE,
            Some("https://zenmux.ai/api/v1"),
            Activation::Auto,
            false,
            &user,
        );
        assert_eq!(injection.headers, pair(false));
        assert!(injection.user_headers_win, "zenmux spreads the pair first");
        let merged = injection.merged_over(&user);
        assert_eq!(
            merged.get("HTTP-Referer").map(String::as_str),
            Some("https://mine.example/"),
            "user header wins"
        );
        assert_eq!(merged.get("X-Title").map(String::as_str), Some("opencode"));
        // zenmux.ts:10 `!==` — trailing slash, prefix, or None all fail.
        for base in [
            None,
            Some("https://zenmux.ai/api/v1/"),
            Some("https://zenmux.ai/api/v1/v2"),
            Some("https://other.test/api/v1"),
        ] {
            assert!(
                plugin_provider_headers(
                    OPENAI_COMPATIBLE,
                    base,
                    Activation::Enabled,
                    false,
                    &existing()
                )
                .headers
                .is_empty(),
                "base {base:?} must not inject"
            );
        }
        // A user HTTP-Referer must not flip another openai-compatible
        // gateway onto the zenmux branch (package+baseURL both gate).
        assert!(
            plugin_provider_headers(
                OPENAI_COMPATIBLE,
                Some("https://api.kilo.ai/api/gateway"),
                Activation::Auto,
                false,
                &user
            )
            .headers
            .get("X-Title")
            .is_some()
        );
    }

    /// G-285③ — vercel keys on the aisdk @ai-sdk/vercel package and injects
    /// the LOWERCASE pair (vercel.ts:13).
    #[test]
    fn vercel_aisdk_package_gets_lowercase_pair() {
        let injection = plugin_provider_headers(VERCEL, None, Activation::Auto, false, &existing());
        assert_eq!(injection.headers, pair(true));
        assert!(!injection.user_headers_win);
        // No other package name injects the lowercase pair.
        for package in [
            "@opencode/ai/providers/vercel-ai-gateway",
            "@ai-sdk/gateway",
            "@opencode/ai/providers/openai-compatible",
        ] {
            assert!(
                plugin_provider_headers(package, None, Activation::Auto, false, &existing())
                    .headers
                    .is_empty(),
                "package {package} must not inject"
            );
        }
    }

    #[test]
    fn kilo_exact_baseurl_gets_referer_and_title() {
        let injection = plugin_provider_headers(
            OPENAI_COMPATIBLE,
            Some("https://api.kilo.ai/api/gateway"),
            Activation::Auto,
            false,
            &existing(),
        );
        assert_eq!(injection.headers, pair(false));
        assert!(!injection.user_headers_win);
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
                .headers
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
            .headers
            .is_empty()
        );
    }

    #[test]
    fn nvidia_defaults_billing_origin_and_keeps_user_value() {
        let injection = plugin_provider_headers(
            OPENAI_COMPATIBLE,
            Some("https://integrate.api.nvidia.com/v1"),
            Activation::Auto,
            false,
            &existing(),
        );
        assert_eq!(
            injection.headers,
            BTreeMap::from([
                ("HTTP-Referer".to_string(), "https://opencode.ai/".to_string()),
                ("X-Title".to_string(), "opencode".to_string()),
                ("X-BILLING-INVOKE-ORIGIN".to_string(), "OpenCode".to_string()),
            ])
        );
        // nvidia.ts:16 — an existing provider header wins over the default.
        let injection = plugin_provider_headers(
            OPENAI_COMPATIBLE,
            Some("https://integrate.api.nvidia.com/v1"),
            Activation::Auto,
            false,
            &BTreeMap::from([("X-BILLING-INVOKE-ORIGIN".to_string(), "custom-origin".to_string())]),
        );
        assert_eq!(
            injection.headers.get("X-BILLING-INVOKE-ORIGIN").map(String::as_str),
            Some("custom-origin")
        );
    }

    #[test]
    fn llmgateway_requires_baseurl_activation_and_registered_integration() {
        let injection = plugin_provider_headers(
            OPENAI_COMPATIBLE,
            Some("https://api.llmgateway.io/v1"),
            Activation::Auto,
            true,
            &existing(),
        );
        assert_eq!(
            injection.headers,
            BTreeMap::from([
                ("HTTP-Referer".to_string(), "https://opencode.ai/".to_string()),
                ("X-Title".to_string(), "opencode".to_string()),
                ("X-Source".to_string(), "opencode".to_string()),
            ])
        );
        // llmgateway.ts:12 — disabled providers are skipped; :15 — no
        // registered integration under the provider id is skipped.
        for (activation, registered) in [(Activation::Disabled, true), (Activation::Auto, false)] {
            assert!(
                plugin_provider_headers(
                    OPENAI_COMPATIBLE,
                    Some("https://api.llmgateway.io/v1"),
                    activation,
                    registered,
                    &existing(),
                )
                .headers
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
                .headers
                .is_empty(),
                "package {package} must not inject"
            );
        }
    }
}
