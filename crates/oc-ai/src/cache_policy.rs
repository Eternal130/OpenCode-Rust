//! Cache-policy application (port of `src/cache-policy.ts`): inject
//! `CacheHint`s at the boundaries the policy designates, once at compile time
//! before the per-protocol body builder. Manual placements are preserved and
//! count against the four-breakpoint budget.

use crate::effort_updates::effort_update;
use crate::schema::{
    CacheHint, CachePolicy, CachePolicyMessages, CachePolicyObject, ContentPart, LLMRequest,
    LLMRequestPatch, Message, MessageRole, SystemPart, ToolDefinition, ToolEntry, ToolNamespace,
};

/// Auto placement: last tool, first/last system parts, conversation tail.
fn auto_policy() -> CachePolicyObject {
    CachePolicyObject {
        tools: Some(true),
        system: Some(true),
        messages: Some(CachePolicyMessages::Tail { tail: 1 }),
        ttl_seconds: None,
    }
}

fn none_policy() -> CachePolicyObject {
    CachePolicyObject::default()
}

pub const BREAKPOINT_CAP: usize = 4;

fn resolve(policy: Option<&CachePolicy>) -> CachePolicyObject {
    match policy {
        None | Some(CachePolicy::Auto) => auto_policy(),
        Some(CachePolicy::None) => none_policy(),
        Some(CachePolicy::Object(object)) => object.clone(),
    }
}

/// Protocols whose wire format ignores inline cache markers (`OpenAI`'s
/// implicit prefix caching, Gemini's implicit + out-of-band `CachedContent`).
/// Skip the whole policy pass for these.
const RESPECTS_INLINE_HINTS: [&str; 15] = [
    "alibaba-chat",
    "alibaba-messages",
    "anthropic-messages",
    "anthropic-compatible-messages",
    "bedrock-mantle-messages",
    "cloudflare-ai-gateway-messages",
    "google-vertex-messages",
    "meta-messages",
    "minimax-messages",
    "moonshot-messages",
    "vercel-ai-gateway-messages",
    "zai-coding-messages",
    "bedrock-converse",
    "openrouter",
    "digitalocean",
];

/// Gemini uses only the last breakpoint, so a conversation-tail breakpoint
/// writes a new cache every step and costs more than none. Qwen ignores
/// breakpoints on tool definitions and caches tools with the system prompt.
fn qwen_policy() -> CachePolicyObject {
    CachePolicyObject {
        tools: None,
        system: Some(true),
        messages: Some(CachePolicyMessages::Tail { tail: 1 }),
        ttl_seconds: None,
    }
}

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

fn make_hint(ttl_seconds: Option<f64>) -> CacheHint {
    match ttl_seconds {
        Some(ttl_seconds) => CacheHint::ephemeral_ttl(ttl_seconds),
        None => CacheHint::ephemeral(),
    }
}

#[derive(Clone, Copy)]
struct Budget {
    remaining: usize,
}

fn mark_last_tool(tools: Vec<ToolEntry>, hint: &CacheHint, budget: &mut Budget) -> Vec<ToolEntry> {
    let Some((last, prefix)) = tools.split_last() else { return tools };
    match last {
        ToolEntry::Namespace(namespace) => {
            let nested = mark_last_tool(namespace.tools.clone(), hint, budget);
            if nested == namespace.tools {
                tools
            } else {
                let mut result = prefix.to_vec();
                result.push(ToolEntry::Namespace(ToolNamespace {
                    tools: nested,
                    ..namespace.clone()
                }));
                result
            }
        }
        ToolEntry::Definition(tool) => {
            if tool.cache.is_some() || budget.remaining == 0 {
                tools
            } else {
                budget.remaining -= 1;
                let mut result = prefix.to_vec();
                result.push(ToolEntry::Definition(ToolDefinition {
                    cache: Some(hint.clone()),
                    ..tool.clone()
                }));
                result
            }
        }
    }
}

fn count_tool_hints(tools: &[ToolEntry]) -> usize {
    tools
        .iter()
        .map(|tool| match tool {
            ToolEntry::Definition(tool) => usize::from(tool.cache.is_some()),
            ToolEntry::Namespace(namespace) => count_tool_hints(&namespace.tools),
        })
        .sum()
}

fn mark_system_boundaries(
    system: Vec<SystemPart>,
    hint: &CacheHint,
    budget: &mut Budget,
) -> Vec<SystemPart> {
    if system.is_empty() {
        return system;
    }
    let len = system.len();
    system
        .into_iter()
        .enumerate()
        .map(|(index, mut part)| {
            // first and last distinct system parts; a single part is both.
            if (index != 0 && index != len - 1) || part.cache.is_some() || budget.remaining == 0 {
                return part;
            }
            budget.remaining -= 1;
            part.cache = Some(hint.clone());
            part
        })
        .collect()
}

fn last_index_of_role(messages: &[Message], role: MessageRole) -> Option<usize> {
    messages.iter().rposition(|message| message.role == role)
}

/// Mark the last text part of `messages[index]`; if no text part exists,
/// mark the last content part regardless of type (port of `markMessageAt`).
fn mark_message_at(
    mut messages: Vec<Message>,
    index: usize,
    hint: &CacheHint,
    budget: &mut Budget,
) -> Vec<Message> {
    if index >= messages.len() {
        return messages;
    }
    let target = &messages[index];
    if target.content.is_empty() {
        return messages;
    }
    let last_text_index =
        target.content.iter().rposition(|part| matches!(part, ContentPart::Text { .. }));
    let mark_at = last_text_index.unwrap_or(target.content.len() - 1);
    if target.content[mark_at].cache().is_some() || budget.remaining == 0 {
        return messages;
    }
    budget.remaining -= 1;
    let content: Vec<ContentPart> = target
        .content
        .iter()
        .enumerate()
        .map(|(i, part)| {
            if i != mark_at {
                return part.clone();
            }
            match part {
                ContentPart::Text { text, metadata, provider_metadata, .. } => ContentPart::Text {
                    text: text.clone(),
                    cache: Some(hint.clone()),
                    metadata: metadata.clone(),
                    provider_metadata: provider_metadata.clone(),
                },
                ContentPart::Media { media, filename, metadata, provider_metadata, .. } => {
                    ContentPart::Media {
                        media: media.clone(),
                        filename: filename.clone(),
                        cache: Some(hint.clone()),
                        metadata: metadata.clone(),
                        provider_metadata: provider_metadata.clone(),
                    }
                }
                ContentPart::ToolCall {
                    id,
                    name,
                    namespace,
                    input,
                    provider_executed,
                    metadata,
                    provider_metadata,
                    ..
                } => ContentPart::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    namespace: namespace.clone(),
                    input: input.clone(),
                    provider_executed: *provider_executed,
                    cache: Some(hint.clone()),
                    metadata: metadata.clone(),
                    provider_metadata: provider_metadata.clone(),
                },
                ContentPart::ToolResult {
                    id,
                    name,
                    namespace,
                    result,
                    provider_executed,
                    metadata,
                    provider_metadata,
                    ..
                } => ContentPart::ToolResult {
                    id: id.clone(),
                    name: name.clone(),
                    namespace: namespace.clone(),
                    result: result.clone(),
                    provider_executed: *provider_executed,
                    cache: Some(hint.clone()),
                    metadata: metadata.clone(),
                    provider_metadata: provider_metadata.clone(),
                },
                ContentPart::Reasoning { text, encrypted, metadata, provider_metadata, .. } => {
                    ContentPart::Reasoning {
                        text: text.clone(),
                        encrypted: encrypted.clone(),
                        cache: Some(hint.clone()),
                        metadata: metadata.clone(),
                        provider_metadata: provider_metadata.clone(),
                    }
                }
                other => other.clone(),
            }
        })
        .collect();
    messages[index] = Message { content, ..target.clone() };
    messages
}

fn mark_messages(
    messages: Vec<Message>,
    strategy: &CachePolicyMessages,
    hint: &CacheHint,
    budget: &mut Budget,
) -> Vec<Message> {
    if messages.is_empty() {
        return messages;
    }
    match strategy {
        CachePolicyMessages::LatestUserMessage => {
            match last_index_of_role(&messages, MessageRole::User) {
                Some(index) => mark_message_at(messages, index, hint, budget),
                None => messages,
            }
        }
        CachePolicyMessages::LatestAssistant => {
            match last_index_of_role(&messages, MessageRole::Assistant) {
                Some(index) => mark_message_at(messages, index, hint, budget),
                None => messages,
            }
        }
        CachePolicyMessages::Tail { tail } => {
            let tail = *tail as usize;
            // Skip effort-update markers when counting tail messages (port of
            // the `effortUpdate` guard).
            let mut start = messages.len();
            let mut remaining = tail;
            while remaining > 0 && start > 0 {
                start -= 1;
                if effort_update(&messages[start]).is_none() {
                    remaining -= 1;
                }
            }
            let mut next = messages;
            for index in start..next.len() {
                if effort_update(&next[index]).is_none() {
                    next = mark_message_at(next, index, hint, budget);
                }
            }
            next
        }
    }
}

fn count_hints(request: &LLMRequest) -> usize {
    count_tool_hints(&request.tools)
        + request.system.iter().filter(|part| part.cache.is_some()).count()
        + request
            .messages
            .iter()
            .map(|message| message.content.iter().filter(|part| part.cache().is_some()).count())
            .sum::<usize>()
}

/// Apply the request's cache policy (port of `applyCachePolicy`). No-op for
/// routes that ignore inline hints.
#[must_use]
pub fn apply_cache_policy(request: &LLMRequest) -> LLMRequest {
    let route = request.model.route.id.as_str();
    if !RESPECTS_INLINE_HINTS.contains(&route) {
        return request.clone();
    }
    let policy = if (route == "openrouter" || route == "vercel-ai-gateway-messages")
        && matches!(request.cache, None | Some(CachePolicy::Auto))
    {
        gateway_policy(&request.model.id.0)
    } else if route == "alibaba-chat" && matches!(request.cache, None | Some(CachePolicy::Auto)) {
        if request.model.id.0.to_lowercase().starts_with("qwen") {
            qwen_policy()
        } else {
            none_policy()
        }
    } else {
        resolve(request.cache.as_ref())
    };
    if !policy.tools.unwrap_or(false)
        && !policy.system.unwrap_or(false)
        && policy.messages.is_none()
    {
        return request.clone();
    }

    let hint = make_hint(policy.ttl_seconds);
    let before = count_hints(request);
    let mut budget = Budget { remaining: BREAKPOINT_CAP.saturating_sub(before) };
    let tools = if policy.tools.unwrap_or(false) {
        mark_last_tool(request.tools.clone(), &hint, &mut budget)
    } else {
        request.tools.clone()
    };
    let system = if policy.system.unwrap_or(false) {
        mark_system_boundaries(request.system.clone(), &hint, &mut budget)
    } else {
        request.system.clone()
    };
    let messages = match &policy.messages {
        Some(strategy) => mark_messages(request.messages.clone(), strategy, &hint, &mut budget),
        None => request.messages.clone(),
    };

    let updated = request_with(request, tools, system, messages);
    if count_hints(&updated) == before {
        request.clone()
    } else {
        updated
    }
}

fn request_with(
    request: &LLMRequest,
    tools: Vec<ToolEntry>,
    system: Vec<SystemPart>,
    messages: Vec<Message>,
) -> LLMRequest {
    request.update(LLMRequestPatch {
        tools: Some(tools),
        system: Some(system),
        messages: Some(messages),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{CachePolicy, ContentPart, Message, SystemPart, ToolDefinition};
    use serde_json::json;

    fn request(route_id: &str, model_id: &str, mut base: LLMRequest) -> LLMRequest {
        // rebind onto a route with the requested id (cache policy consults route id)
        let route = crate::test_support::echo_route_named(route_id);
        base.model = crate::schema::LanguageModel::new(model_id, "test", route);
        base
    }

    fn base_request() -> LLMRequest {
        let mut req = crate::test_support::request_for_tests(Vec::new());
        req.system = vec![SystemPart::new("You are concise.")];
        req.messages = vec![Message::user("hi")];
        req
    }

    fn hints_on_tools(tools: &[ToolEntry]) -> usize {
        count_tool_hints(tools)
    }

    #[test]
    fn undefined_cache_resolves_to_auto() {
        let req = request("anthropic-messages", "claude-sonnet-4-5", base_request());
        let prepared = apply_cache_policy(&req);
        // single system block is both first and last boundary → marked once;
        // the conversation tail is also marked
        assert!(prepared.system[0].cache.is_some());
        match &prepared.messages[0].content[0] {
            ContentPart::Text { cache, .. } => assert!(cache.is_some()),
            other => panic!("expected text, got {}", other.kind()),
        }
    }

    #[test]
    fn auto_marks_last_tool_first_last_system_and_tail() {
        let mut req = base_request();
        req.system = vec![SystemPart::new("Base agent"), SystemPart::new("Project instructions")];
        req.tools =
            vec![ToolEntry::Definition(ToolDefinition::new("t1", "t1", json!({"type": "object"})))];
        req.messages = vec![
            Message::user("first user"),
            Message::assistant(vec![ContentPart::text("assistant reply")]),
            Message::user("latest user message"),
        ];
        req.cache = Some(CachePolicy::Auto);
        let prepared = apply_cache_policy(&request("anthropic-messages", "claude-sonnet-4-5", req));
        assert_eq!(hints_on_tools(&prepared.tools), 1);
        assert!(prepared.system[0].cache.is_some());
        assert!(prepared.system[1].cache.is_some());
        assert!(prepared.messages[0].content[0].cache().is_none());
        assert!(prepared.messages[1].content[0].cache().is_none());
        assert!(prepared.messages[2].content[0].cache().is_some());
        // total breakpoints: 1 tool + 2 system + 1 tail = 4 (the cap)
        assert_eq!(count_hints(&prepared), BREAKPOINT_CAP);
    }

    #[test]
    fn none_leaves_everything_unmarked() {
        let mut req = base_request();
        req.cache = Some(CachePolicy::None);
        let prepared = apply_cache_policy(&request("anthropic-messages", "claude-sonnet-4-5", req));
        assert!(prepared.system[0].cache.is_none());
        assert!(prepared.messages[0].content[0].cache().is_none());
    }

    #[test]
    fn object_form_overrides_placement() {
        let mut req = base_request();
        req.system = vec![SystemPart::new("a"), SystemPart::new("b")];
        req.messages = vec![Message::user("one"), Message::user("two")];
        req.cache = Some(CachePolicy::Object(CachePolicyObject {
            tools: Some(false),
            system: Some(true),
            messages: Some(CachePolicyMessages::LatestAssistant),
            ttl_seconds: Some(300.0),
        }));
        let prepared = apply_cache_policy(&request("anthropic-messages", "claude-sonnet-4-5", req));
        assert!(prepared.system.iter().all(|part| part.cache.is_some()));
        // no assistant message exists → LatestAssistant marks nothing
        assert!(prepared.messages.iter().all(|m| m.content.iter().all(|p| p.cache().is_none())));
        // ttl flows into the hint
        assert_eq!(prepared.system[0].cache.as_ref().unwrap().ttl_seconds, Some(300.0));
    }

    #[test]
    fn manual_hints_count_against_the_budget() {
        let mut req = base_request();
        req.system = vec![SystemPart::new("a")];
        req.messages = vec![Message::user("one"), Message::user("two"), Message::user("three")];
        // burn the entire budget manually on the first two messages
        req.messages[0].content[0] = match &req.messages[0].content[0] {
            ContentPart::Text { text, .. } => ContentPart::Text {
                text: text.clone(),
                cache: Some(CacheHint::ephemeral()),
                metadata: None,
                provider_metadata: None,
            },
            other => other.clone(),
        };
        req.messages[1].content[0] = match &req.messages[1].content[0] {
            ContentPart::Text { text, .. } => ContentPart::Text {
                text: text.clone(),
                cache: Some(CacheHint::ephemeral()),
                metadata: None,
                provider_metadata: None,
            },
            other => other.clone(),
        };
        let prepared = apply_cache_policy(&request("anthropic-messages", "claude-sonnet-4-5", req));
        // 2 slots remain: the single system boundary + the tail; tools mark nothing
        assert!(prepared.system.iter().all(|part| part.cache.is_some()));
        assert!(prepared.messages[2].content[0].cache().is_some());
        assert_eq!(count_hints(&prepared), BREAKPOINT_CAP);
    }

    #[test]
    fn routes_without_inline_hints_skip_the_pass_entirely() {
        let req = request("openai-chat", "gpt-4o-mini", base_request());
        let prepared = apply_cache_policy(&req);
        assert!(prepared.system[0].cache.is_none());
        assert!(prepared.messages[0].content[0].cache().is_none());
    }

    #[test]
    fn gemini_uses_only_the_tail_breakpoint() {
        let req = request("gemini", "gemini-2.5-flash", base_request());
        // gemini is not in RESPECTS_INLINE_HINTS → skip
        let prepared = apply_cache_policy(&req);
        assert!(prepared.system[0].cache.is_none());
    }

    #[test]
    fn openrouter_gateway_policy_follows_the_model_alias() {
        let req = request("openrouter", "~anthropic/claude-sonnet-latest", base_request());
        let prepared = apply_cache_policy(&req);
        assert!(prepared.system[0].cache.is_some(), "anthropic aliases keep the auto policy");
        assert!(prepared.messages[0].content[0].cache().is_some());

        let qwen = apply_cache_policy(&request("openrouter", "qwen/qwen3-coder", base_request()));
        assert!(qwen.system[0].cache.is_some(), "qwen keeps system + tail");
        assert!(qwen.messages[0].content[0].cache().is_some());
        assert_eq!(count_hints(&qwen), 2);

        let other =
            apply_cache_policy(&request("openrouter", "meta-llama/llama-4", base_request()));
        assert_eq!(count_hints(&other), 0, "other upstreams cache without breakpoints");
    }

    #[test]
    fn alibaba_chat_qwen_models_cache_with_system_only() {
        let qwen = apply_cache_policy(&request("alibaba-chat", "Qwen3-Max", base_request()));
        assert!(qwen.system[0].cache.is_some(), "qwen caches with the system prompt");
        assert!(qwen.messages[0].content[0].cache().is_some(), "qwen keeps the conversation tail");
        let other = apply_cache_policy(&request("alibaba-chat", "deepseek-v3", base_request()));
        assert_eq!(count_hints(&other), 0);
    }

    #[test]
    fn marks_the_final_leaf_inside_a_tool_namespace() {
        let mut req = base_request();
        req.tools = vec![ToolEntry::Namespace(ToolNamespace {
            name: "ns".into(),
            description: None,
            tools: vec![
                ToolEntry::Definition(ToolDefinition::new("a", "a", json!({"type": "object"}))),
                ToolEntry::Definition(ToolDefinition::new("b", "b", json!({"type": "object"}))),
            ],
        })];
        let prepared = apply_cache_policy(&request("anthropic-messages", "claude-sonnet-4-5", req));
        match &prepared.tools[0] {
            ToolEntry::Namespace(namespace) => {
                assert!(namespace.tools[0].as_definition().unwrap().cache.is_none());
                assert!(namespace.tools[1].as_definition().unwrap().cache.is_some());
            }
            other => panic!("expected namespace, got {other:?}"),
        }
    }

    #[test]
    fn effort_markers_do_not_consume_tail_slots() {
        let mut req = base_request();
        req.messages = vec![
            Message::user("one"),
            Message::effort(Some("low".into()), Some("high".into())),
            Message::user("two"),
        ];
        req.cache = Some(CachePolicy::Object(CachePolicyObject {
            messages: Some(CachePolicyMessages::Tail { tail: 1 }),
            ..Default::default()
        }));
        let prepared = apply_cache_policy(&request("anthropic-messages", "claude-sonnet-4-5", req));
        assert!(prepared.messages[0].content[0].cache().is_none());
        assert!(prepared.messages[1].content[0].cache().is_none(), "markers are not breakpoints");
        assert!(prepared.messages[2].content[0].cache().is_some());
    }
}
