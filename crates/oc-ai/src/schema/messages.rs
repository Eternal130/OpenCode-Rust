//! Messages, parts, tools and the common request (port of
//! `src/schema/messages.ts`). Wire shapes are camelCase with kebab-case
//! `type` tags, matching the TS contract exactly.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ids::ProviderId;
use super::media::Asset;
use super::options::{
    CacheHint, CachePolicy, GenerationOptions, HttpOptions, ProviderMetadata, ProviderOptions,
};

/// Chat role of one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// System-prompt part (`type: "text"` in the system array).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename = "text")]
pub struct SystemPart {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheHint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, Value>>,
}

impl SystemPart {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), cache: None, metadata: None }
    }
}

/// Tool result payload: `json`/`text`/`error` carry an unknown value;
/// `content` carries a `Tool.Content` array (kept as JSON here).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ToolResultValue {
    Json {
        value: Value,
    },
    Text {
        value: Value,
    },
    Error {
        value: Value,
    },
    /// `value` items are `Tool.Content` entries (see [`ToolContentItems`]).
    Content {
        value: ToolContentItems,
    },
}

/// `Tool.Content` array with minimal wire validation matching the TS schema
/// guard: items must be objects, and `text` items must carry a string
/// `text`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolContentItems(pub Vec<Value>);

impl Serialize for ToolContentItems {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ToolContentItems {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let items = Vec::<Value>::deserialize(deserializer)?;
        for item in &items {
            let Some(object) = item.as_object() else {
                return Err(serde::de::Error::custom("content items must be objects"));
            };
            let kind = object.get("type").and_then(|value| value.as_str()).unwrap_or_default();
            if kind == "text" && !object.get("text").is_some_and(serde_json::Value::is_string) {
                return Err(serde::de::Error::custom("text content items require a string `text`"));
            }
        }
        Ok(ToolContentItems(items))
    }
}

impl ToolResultValue {
    /// Normalize arbitrary result input (port of `ToolResultValue.make`).
    #[must_use]
    pub fn make(value: Value, kind: &str) -> Self {
        match kind {
            "content" => ToolResultValue::Content {
                value: ToolContentItems(value.as_array().cloned().unwrap_or_default()),
            },
            "text" => ToolResultValue::Text { value },
            "error" => ToolResultValue::Error { value },
            _ => ToolResultValue::Json { value },
        }
    }

    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            ToolResultValue::Json { .. } => "json",
            ToolResultValue::Text { .. } => "text",
            ToolResultValue::Error { .. } => "error",
            ToolResultValue::Content { .. } => "content",
        }
    }
}

/// Structured + content output of a locally executed tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutput {
    pub structured: Value,
    pub content: Vec<Value>,
}

/// One message content part (port of `ContentPart` union).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<CacheHint>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<BTreeMap<String, Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "media")]
    Media {
        media: Asset,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<CacheHint>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<BTreeMap<String, Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-call")]
    ToolCall {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        input: Value,
        #[serde(default, rename = "providerExecuted", skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<CacheHint>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<BTreeMap<String, Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-result")]
    ToolResult {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        result: ToolResultValue,
        #[serde(default, rename = "providerExecuted", skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<CacheHint>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<BTreeMap<String, Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "reasoning")]
    Reasoning {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<CacheHint>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<BTreeMap<String, Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "compaction")]
    Compaction {
        provider: ProviderId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Exactly one of `encrypted` / `text` must be present (`text: null`
        /// means the provider failed to produce a summary).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    #[serde(rename = "effort")]
    Effort {
        /// `None` serializes as absent — the model default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous: Option<String>,
    },
}

impl ContentPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into(), cache: None, metadata: None, provider_metadata: None }
    }

    /// Inline cache hint, when this part carries one.
    #[must_use]
    pub fn cache(&self) -> Option<&CacheHint> {
        match self {
            ContentPart::Text { cache, .. }
            | ContentPart::Media { cache, .. }
            | ContentPart::ToolCall { cache, .. }
            | ContentPart::ToolResult { cache, .. }
            | ContentPart::Reasoning { cache, .. } => cache.as_ref(),
            ContentPart::Compaction { .. } | ContentPart::Effort { .. } => None,
        }
    }

    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            ContentPart::Text { .. } => "text",
            ContentPart::Media { .. } => "media",
            ContentPart::ToolCall { .. } => "tool-call",
            ContentPart::ToolResult { .. } => "tool-result",
            ContentPart::Reasoning { .. } => "reasoning",
            ContentPart::Compaction { .. } => "compaction",
            ContentPart::Effort { .. } => "effort",
        }
    }
}

/// Validate the compaction XOR invariant: either encrypted content or a
/// summary (which may be `null`), never both.
pub fn validate_compaction(encrypted: Option<&str>, text: Option<&String>) -> Result<(), String> {
    match (encrypted, text) {
        (Some(_), Some(_)) => {
            Err("Compaction requires either encrypted content or a summary, not both".into())
        }
        (None, None) => Err("Compaction requires either encrypted content or a summary".into()),
        _ => Ok(()),
    }
}

/// A provider-generated context checkpoint, distinct from visible text.
impl ContentPart {
    pub fn compaction_encrypted(provider: impl Into<String>, encrypted: impl Into<String>) -> Self {
        Self::Compaction {
            provider: ProviderId::new(provider),
            id: None,
            encrypted: Some(encrypted.into()),
            text: None,
        }
    }

    pub fn compaction_summary(provider: impl Into<String>, text: Option<String>) -> Self {
        Self::Compaction { provider: ProviderId::new(provider), id: None, encrypted: None, text }
    }
}

/// One chat message (port of `Message`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub role: MessageRole,
    pub content: Vec<ContentPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<BTreeMap<String, Value>>,
}

impl Message {
    #[must_use]
    pub fn new(role: MessageRole, content: Vec<ContentPart>) -> Self {
        Self { id: None, role, content, metadata: None, provider_metadata: None, native: None }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self::new(MessageRole::User, vec![ContentPart::text(text)])
    }

    #[must_use]
    pub fn assistant(content: Vec<ContentPart>) -> Self {
        Self::new(MessageRole::Assistant, content)
    }

    /// Operator-authored instruction at this chronological point (distinct
    /// from the initial system prompt).
    pub fn system(text: impl Into<String>) -> Self {
        Self::new(MessageRole::System, vec![ContentPart::text(text)])
    }

    /// Effort-change marker: `previous` → `effort`; `None` is the model default.
    #[must_use]
    pub fn effort(effort: Option<String>, previous: Option<String>) -> Self {
        Self::new(MessageRole::System, vec![ContentPart::Effort { effort, previous }])
    }

    #[must_use]
    pub fn tool(part: ContentPart) -> Self {
        Self::new(MessageRole::Tool, vec![part])
    }

    pub fn tool_call(id: impl Into<String>, name: impl Into<String>, input: Value) -> Self {
        Self::assistant(vec![ContentPart::ToolCall {
            id: id.into(),
            name: name.into(),
            namespace: None,
            input,
            provider_executed: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        }])
    }

    pub fn tool_result(
        id: impl Into<String>,
        name: impl Into<String>,
        result: ToolResultValue,
    ) -> Self {
        Self::tool(ContentPart::ToolResult {
            id: id.into(),
            name: name.into(),
            namespace: None,
            result,
            provider_executed: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        })
    }
}

/// A tool definition (port of `ToolDefinition`; `type: "tool"` on the wire).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename = "tool", rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool input.
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "outputSchema")]
    pub output_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheHint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, Value>>,
    /// Provider-specific hosted-tool config keyed by provider name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<BTreeMap<String, Value>>,
}

impl ToolDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            output_schema: None,
            cache: None,
            metadata: None,
            native: None,
        }
    }
}

/// Namespaced tool group; leaf names join their namespace path later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename = "namespace")]
pub struct ToolNamespace {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub tools: Vec<ToolEntry>,
}

/// A flat tool or a namespace (port of `ToolEntry`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ToolEntry {
    #[serde(rename = "tool")]
    Definition(ToolDefinition),
    #[serde(rename = "namespace")]
    Namespace(ToolNamespace),
}

impl ToolEntry {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            ToolEntry::Definition(tool) => &tool.name,
            ToolEntry::Namespace(namespace) => &namespace.name,
        }
    }

    /// The flat tool definition, when this entry is one.
    #[must_use]
    pub fn as_definition(&self) -> Option<&ToolDefinition> {
        match self {
            ToolEntry::Definition(tool) => Some(tool),
            ToolEntry::Namespace(_) => None,
        }
    }
}

/// Tool selection mode (port of `ToolChoice`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolChoice {
    #[serde(rename = "auto")]
    Auto {
        #[serde(
            default,
            rename = "disableParallelToolUse",
            skip_serializing_if = "Option::is_none"
        )]
        disable_parallel_tool_use: Option<bool>,
    },
    #[serde(rename = "none")]
    None {},
    #[serde(rename = "required")]
    Required {
        #[serde(
            default,
            rename = "disableParallelToolUse",
            skip_serializing_if = "Option::is_none"
        )]
        disable_parallel_tool_use: Option<bool>,
    },
    #[serde(rename = "tool")]
    Named {
        name: String,
        #[serde(
            default,
            rename = "disableParallelToolUse",
            skip_serializing_if = "Option::is_none"
        )]
        disable_parallel_tool_use: Option<bool>,
    },
}

impl ToolChoice {
    #[must_use]
    pub fn auto() -> Self {
        ToolChoice::Auto { disable_parallel_tool_use: None }
    }

    #[must_use]
    pub fn none() -> Self {
        ToolChoice::None {}
    }

    #[must_use]
    pub fn required() -> Self {
        ToolChoice::Required { disable_parallel_tool_use: None }
    }

    pub fn named(name: impl Into<String>) -> Self {
        ToolChoice::Named { name: name.into(), disable_parallel_tool_use: None }
    }
}

/// The common provider-neutral request (port of `LLMRequest`). The model is
/// behavioral (carries the configured route), so this type is not
/// serializable — wire bodies are protocol-native.
#[derive(Clone)]
pub struct LLMRequest {
    pub id: Option<String>,
    pub model: LanguageModelRef,
    pub system: Vec<SystemPart>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolEntry>,
    pub tool_choice: Option<ToolChoice>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<ProviderOptions>,
    pub http: Option<HttpOptions>,
    pub cache: Option<CachePolicy>,
    /// Stable cache affinity for protocols with provider-managed prompt caching.
    pub prompt_cache_key: Option<String>,
    pub metadata: Option<BTreeMap<String, Value>>,
}

pub use super::options::LanguageModel as LanguageModelRef;

impl std::fmt::Debug for LLMRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LLMRequest")
            .field("id", &self.id)
            .field("model", &self.model)
            .field("messages", &self.messages.len())
            .field("tools", &self.tools.len())
            .finish_non_exhaustive()
    }
}

impl LLMRequest {
    #[must_use]
    pub fn new(model: LanguageModelRef) -> Self {
        Self {
            id: None,
            model,
            system: Vec::new(),
            messages: Vec::new(),
            tools: Vec::new(),
            tool_choice: None,
            generation: None,
            provider_options: None,
            http: None,
            cache: None,
            prompt_cache_key: None,
            metadata: None,
        }
    }

    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.messages.push(Message::user(prompt));
        self
    }

    pub fn with_system(mut self, text: impl Into<String>) -> Self {
        self.system.push(SystemPart::new(text));
        self
    }

    #[must_use]
    pub fn with_message(mut self, message: Message) -> Self {
        self.messages.push(message);
        self
    }

    #[must_use]
    pub fn with_tool(mut self, tool: ToolDefinition) -> Self {
        self.tools.push(ToolEntry::Definition(tool));
        self
    }

    #[must_use]
    pub fn with_generation(mut self, generation: GenerationOptions) -> Self {
        self.generation = Some(generation);
        self
    }

    /// Immutable patch; `None` fields keep their current value (port of
    /// `LLMRequest.update`).
    #[must_use]
    pub fn update(&self, patch: LLMRequestPatch) -> Self {
        let mut next = self.clone();
        let LLMRequestPatch {
            id,
            system,
            messages,
            tools,
            tool_choice,
            generation,
            provider_options,
            http,
            cache,
            prompt_cache_key,
            metadata,
        } = patch;
        if let Some(id) = id {
            next.id = id;
        }
        if let Some(system) = system {
            next.system = system;
        }
        if let Some(messages) = messages {
            next.messages = messages;
        }
        if let Some(tools) = tools {
            next.tools = tools;
        }
        if let Some(tool_choice) = tool_choice {
            next.tool_choice = Some(tool_choice);
        }
        if let Some(generation) = generation {
            next.generation = Some(generation);
        }
        if let Some(provider_options) = provider_options {
            next.provider_options = Some(provider_options);
        }
        if let Some(http) = http {
            next.http = Some(http);
        }
        if let Some(cache) = cache {
            next.cache = Some(cache);
        }
        if let Some(prompt_cache_key) = prompt_cache_key {
            next.prompt_cache_key = Some(prompt_cache_key);
        }
        if let Some(metadata) = metadata {
            next.metadata = Some(metadata);
        }
        next
    }
}

/// Patch accepted by [`LLMRequest::update`].
#[derive(Default)]
pub struct LLMRequestPatch {
    pub id: Option<Option<String>>,
    pub system: Option<Vec<SystemPart>>,
    pub messages: Option<Vec<Message>>,
    pub tools: Option<Vec<ToolEntry>>,
    pub tool_choice: Option<ToolChoice>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<ProviderOptions>,
    pub http: Option<HttpOptions>,
    pub cache: Option<CachePolicy>,
    pub prompt_cache_key: Option<String>,
    pub metadata: Option<BTreeMap<String, Value>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_result_value_guard_matches_canonical_shapes() {
        let cases: Vec<(Value, bool)> = vec![
            (serde_json::json!({"type": "json", "value": {"ok": true}}), true),
            (serde_json::json!({"type": "text", "value": "done"}), true),
            (serde_json::json!({"type": "error", "value": "failed"}), true),
            (
                serde_json::json!({"type": "content", "value": [{"type": "text", "text": "done"}]}),
                true,
            ),
            (serde_json::json!({"type": "content", "value": [{"type": "text"}]}), false),
            (serde_json::json!({"type": "content", "value": "done"}), false),
            (serde_json::json!({"type": "json"}), false),
            (serde_json::json!({"type": "unknown", "value": "done"}), false),
        ];
        // "rejects" cases here mean serde fails to produce a ToolResultValue
        // or the value does not equal a canonical decode.
        for (value, expected) in cases {
            let decoded = serde_json::from_value::<ToolResultValue>(value.clone()).is_ok();
            assert_eq!(decoded, expected, "mismatch for {value}");
        }
    }

    #[test]
    fn content_part_union_round_trips_with_kebab_tags() {
        let part = ContentPart::ToolCall {
            id: "call_1".into(),
            name: "lookup".into(),
            namespace: None,
            input: serde_json::json!({"query": "x"}),
            provider_executed: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        };
        let json = serde_json::to_value(&part).unwrap();
        assert_eq!(json["type"], "tool-call");
        assert_eq!(json["input"]["query"], "x");
        let back: ContentPart = serde_json::from_value(json).unwrap();
        assert_eq!(back, part);

        let effort =
            ContentPart::Effort { effort: Some("low".into()), previous: Some("high".into()) };
        let json = serde_json::to_value(&effort).unwrap();
        assert_eq!(json["type"], "effort");
        assert_eq!(json["effort"], "low");
    }

    #[test]
    fn compaction_requires_exactly_one_of_encrypted_or_text() {
        assert!(validate_compaction(Some("enc"), None).is_ok());
        assert!(validate_compaction(None, Some(&"summary".into())).is_ok());
        assert!(validate_compaction(None, None).is_err());
        assert!(validate_compaction(Some("enc"), Some(&"summary".into())).is_err());
    }

    #[test]
    fn messages_round_trip() {
        let message = Message::new(
            MessageRole::Assistant,
            vec![
                ContentPart::Reasoning {
                    text: "thinking".into(),
                    encrypted: Some("sig".into()),
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
                ContentPart::text("answer"),
            ],
        );
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["role"], "assistant");
        assert_eq!(json["content"][0]["type"], "reasoning");
        assert_eq!(json["content"][0]["encrypted"], "sig");
        let back: Message = serde_json::from_value(json).unwrap();
        assert_eq!(back, message);
    }

    #[test]
    fn tool_definition_wire_shape_uses_input_schema_camel_case() {
        let tool =
            ToolDefinition::new("bash", "Run a command", serde_json::json!({"type": "object"}));
        let json = serde_json::to_value(&tool).unwrap();
        assert_eq!(json["type"], "tool");
        assert_eq!(json["inputSchema"]["type"], "object");
        let back: ToolDefinition = serde_json::from_value(json).unwrap();
        assert_eq!(back, tool);
    }

    #[test]
    fn tool_namespaces_nest_entries() {
        let entry = ToolEntry::Namespace(ToolNamespace {
            name: "search".into(),
            description: Some("Search tools".into()),
            tools: vec![ToolEntry::Definition(ToolDefinition::new(
                "web",
                "Web search",
                serde_json::json!({"type": "object"}),
            ))],
        });
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["type"], "namespace");
        assert_eq!(json["tools"][0]["type"], "tool");
        assert_eq!(json["tools"][0]["name"], "web");
        let back: ToolEntry = serde_json::from_value(json).unwrap();
        assert_eq!(back, entry);
    }
}
