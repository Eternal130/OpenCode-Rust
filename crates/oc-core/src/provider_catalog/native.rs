//! Native package resolution (port of `packages/core/src/aisdk-native.ts`
//! tables): maps AI-SDK npm packages onto the `@opencode/ai` native provider
//! entrypoints, per provider host and model shape.

/// v2 `Provider.aisdk` prefix for packages without a native counterpart.
pub const AISDK_PREFIX: &str = "aisdk:";

/// Wrap a package name into its `aisdk:` form (identity if already prefixed).
#[must_use]
pub fn aisdk(package: &str) -> String {
    if package.starts_with(AISDK_PREFIX) {
        package.to_string()
    } else {
        format!("{AISDK_PREFIX}{package}")
    }
}

/// `Provider.packageName`: unwrap the `aisdk:` prefix (and the legacy
/// `@opencode-ai/ai/` scope migration).
#[must_use]
pub fn package_name(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("@opencode-ai/ai/") {
        return format!("@opencode/ai/{rest}");
    }
    if let Some(rest) = value.strip_prefix(AISDK_PREFIX) {
        return rest.to_string();
    }
    value.to_string()
}

/// Generic package → native entrypoint (v2 `PACKAGES`).
fn packages(npm: &str) -> Option<&'static str> {
    Some(match npm {
        "@ai-sdk/amazon-bedrock" => "@opencode/ai/providers/amazon-bedrock",
        "@ai-sdk/alibaba" => "@opencode/ai/providers/alibaba/chat",
        "@ai-sdk/anthropic" => "@opencode/ai/providers/anthropic",
        "@ai-sdk/azure" => "@opencode/ai/providers/azure/responses",
        "@ai-sdk/cerebras" => "@opencode/ai/providers/cerebras",
        "@ai-sdk/cohere" => "@opencode/ai/providers/cohere",
        "@ai-sdk/deepinfra" => "@opencode/ai/providers/deepinfra",
        "@ai-sdk/google" => "@opencode/ai/providers/google",
        "@ai-sdk/google-vertex" => "@opencode/ai/providers/google-vertex",
        "@ai-sdk/google-vertex/anthropic" => "@opencode/ai/providers/google-vertex/messages",
        "@ai-sdk/groq" => "@opencode/ai/providers/groq",
        "@ai-sdk/mistral" => "@opencode/ai/providers/mistral",
        "@ai-sdk/openai" => "@opencode/ai/providers/openai",
        "@ai-sdk/openai-compatible" => "@opencode/ai/providers/openai-compatible",
        "@ai-sdk/togetherai" => "@opencode/ai/providers/togetherai",
        "@ai-sdk/xai" => "@opencode/ai/providers/xai",
        "@ai-sdk/gateway" => "@opencode/ai/providers/vercel-ai-gateway",
        "@openrouter/ai-sdk-provider" => "@opencode/ai/providers/openrouter",
        "ai-gateway-provider" => "@opencode/ai/providers/cloudflare-ai-gateway",
        "venice-ai-sdk-provider" => "@opencode/ai/providers/venice",
        _ => return None,
    })
}

/// v2 `protocols(name)`: the generic AI-SDK packages route onto a host's
/// chat/messages/responses protocol packages.
fn protocols(name: &str, npm: &str) -> Option<String> {
    let suffix = match npm {
        "@ai-sdk/openai-compatible" => "chat",
        "@ai-sdk/anthropic" => "messages",
        "@ai-sdk/openai" => "responses",
        _ => return None,
    };
    Some(format!("@opencode/ai/providers/{name}/{suffix}"))
}

/// Bedrock mantle chat/responses split by model id.
fn mantle(model_id: Option<&str>) -> &'static str {
    match model_id {
        None => "@opencode/ai/providers/amazon-bedrock/mantle",
        Some(model) if model.contains("gpt-oss") => {
            "@opencode/ai/providers/amazon-bedrock/mantle/chat"
        }
        Some(_) => "@opencode/ai/providers/amazon-bedrock/mantle/responses",
    }
}

/// Host table entry kinds (v2 `HOSTS[providerID][npm]`).
enum HostEntry {
    /// `protocols(name)` — the three generic packages.
    Protocols(&'static str),
    /// A single fixed redirect for one package.
    Fixed(&'static str, &'static str),
    /// Several fixed redirects (cloudflare-ai-gateway).
    Many(&'static [(&'static str, &'static str)]),
}

fn host_table(provider_id: &str) -> Option<HostEntry> {
    Some(match provider_id {
        "alibaba"
        | "alibaba-cn"
        | "alibaba-coding-plan"
        | "alibaba-coding-plan-cn"
        | "alibaba-token-plan"
        | "alibaba-token-plan-cn" => HostEntry::Protocols("alibaba"),
        "baseten" => {
            HostEntry::Fixed("@ai-sdk/openai-compatible", "@opencode/ai/providers/baseten")
        }
        "cloudflare-ai-gateway" => HostEntry::Many(&[
            ("@ai-sdk/anthropic", "@opencode/ai/providers/cloudflare-ai-gateway"),
            ("@ai-sdk/openai", "@opencode/ai/providers/cloudflare-ai-gateway"),
            ("@ai-sdk/openai-compatible", "@opencode/ai/providers/cloudflare-ai-gateway"),
            ("ai-gateway-provider", "@opencode/ai/providers/cloudflare-ai-gateway"),
        ]),
        "cohere" => {
            HostEntry::Fixed("@ai-sdk/openai-compatible", "@opencode/ai/providers/cohere/chat")
        }
        "cloudflare-workers-ai" => HostEntry::Fixed(
            "@ai-sdk/openai-compatible",
            "@opencode/ai/providers/cloudflare-workers-ai",
        ),
        "deepseek" => {
            HostEntry::Fixed("@ai-sdk/openai-compatible", "@opencode/ai/providers/deepseek")
        }
        "digitalocean" => {
            HostEntry::Fixed("@ai-sdk/openai-compatible", "@opencode/ai/providers/digitalocean")
        }
        "fireworks-ai" => {
            HostEntry::Fixed("@ai-sdk/openai-compatible", "@opencode/ai/providers/fireworks")
        }
        "google-vertex" => HostEntry::Fixed(
            "@ai-sdk/openai-compatible",
            "@opencode/ai/providers/google-vertex/chat",
        ),
        "kimi-for-coding" | "moonshotai" | "moonshotai-cn" => HostEntry::Protocols("moonshot"),
        "meta" => HostEntry::Protocols("meta"),
        "minimax" | "minimax-cn" | "minimax-coding-plan" | "minimax-cn-coding-plan" => {
            HostEntry::Protocols("minimax")
        }
        "zai" | "zhipuai" => {
            HostEntry::Fixed("@ai-sdk/openai-compatible", "@opencode/ai/providers/zai/chat")
        }
        "zai-coding-plan" | "zhipuai-coding-plan" => HostEntry::Protocols("zai-coding-plan"),
        _ => return None,
    })
}

fn host_lookup(provider_id: &str, npm: &str) -> Option<String> {
    match host_table(provider_id)? {
        HostEntry::Protocols(name) => protocols(name, npm),
        HostEntry::Fixed(package, target) => (npm == package).then(|| target.to_string()),
        HostEntry::Many(entries) => entries
            .iter()
            .find(|(package, _)| *package == npm)
            .map(|(_, target)| (*target).to_string()),
    }
}

/// v2 `azureChat`: the Responses package switches to Chat when the model
/// declares the completions shape (the settings-based variant is not
/// consulted in catalog derivation).
fn azure_chat(shape: Option<&str>) -> bool {
    shape == Some("completions")
}

/// Resolve the native package for `npm` in the context of one provider/model
/// (v2 `AISDKNative.native`). Returns `None` when there is no native
/// counterpart — callers fall back to [`aisdk`].
#[must_use]
pub fn native(
    npm: &str,
    provider_id: &str,
    model_id: Option<&str>,
    shape: Option<&str>,
) -> Option<String> {
    if let Some(host) = host_lookup(provider_id, npm) {
        return Some(host);
    }
    if npm == "@ai-sdk/amazon-bedrock/mantle" {
        return Some(mantle(model_id).to_string());
    }
    if npm == "@ai-sdk/azure" && azure_chat(shape) {
        return Some("@opencode/ai/providers/azure/chat".to_string());
    }
    packages(npm).map(str::to_string)
}

/// The full resolution used by catalog normalization (v2 `nativePackage`):
/// native entrypoint, else `aisdk:<npm>`.
#[must_use]
pub fn native_package(
    npm: &str,
    provider_id: &str,
    model_id: Option<&str>,
    shape: Option<&str>,
) -> String {
    native(npm, provider_id, model_id, shape).unwrap_or_else(|| aisdk(npm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_native_anthropic_and_openai() {
        assert_eq!(
            native_package("@ai-sdk/anthropic", "anthropic", None, None),
            "@opencode/ai/providers/anthropic"
        );
        assert_eq!(
            native_package("@ai-sdk/openai", "openai", None, None),
            "@opencode/ai/providers/openai"
        );
    }

    #[test]
    fn provider_native_host_redirects_alibaba() {
        assert_eq!(
            native_package("@ai-sdk/openai-compatible", "alibaba", Some("qwen-max"), None),
            "@opencode/ai/providers/alibaba/chat"
        );
        assert_eq!(
            native_package("@ai-sdk/anthropic", "alibaba", Some("qwen-max"), None),
            "@opencode/ai/providers/alibaba/messages"
        );
        assert_eq!(
            native_package("@ai-sdk/openai", "alibaba", Some("qwen-max"), None),
            "@opencode/ai/providers/alibaba/responses"
        );
    }

    #[test]
    fn provider_native_mantle_splits_by_model() {
        assert_eq!(
            native_package("@ai-sdk/amazon-bedrock/mantle", "amazon-bedrock", None, None),
            "@opencode/ai/providers/amazon-bedrock/mantle"
        );
        assert_eq!(
            native_package(
                "@ai-sdk/amazon-bedrock/mantle",
                "amazon-bedrock",
                Some("openai.gpt-oss-120b"),
                None
            ),
            "@opencode/ai/providers/amazon-bedrock/mantle/chat"
        );
        assert_eq!(
            native_package(
                "@ai-sdk/amazon-bedrock/mantle",
                "amazon-bedrock",
                Some("anthropic.claude-4"),
                None
            ),
            "@opencode/ai/providers/amazon-bedrock/mantle/responses"
        );
    }

    #[test]
    fn provider_native_azure_shape_split() {
        assert_eq!(
            native_package("@ai-sdk/azure", "azure", Some("gpt"), None),
            "@opencode/ai/providers/azure/responses"
        );
        assert_eq!(
            native_package("@ai-sdk/azure", "azure", Some("gpt"), Some("completions")),
            "@opencode/ai/providers/azure/chat"
        );
    }

    #[test]
    fn provider_native_falls_back_to_aisdk_prefix() {
        assert_eq!(
            native_package("@ai-sdk/perplexity", "perplexity", None, None),
            "aisdk:@ai-sdk/perplexity"
        );
        assert_eq!(aisdk("aisdk:x"), "aisdk:x");
        assert_eq!(package_name("aisdk:x"), "x");
        assert_eq!(package_name("@opencode-ai/ai/x"), "@opencode/ai/x");
    }
}
