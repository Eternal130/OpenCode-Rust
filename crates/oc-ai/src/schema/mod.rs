//! Schema layer: the provider-neutral wire contract (port of
//! `packages/ai/src/schema/`). See module docs on each file for the TS
//! anchor.

pub mod errors;
pub mod events;
pub mod ids;
pub mod media;
pub mod messages;
pub mod options;

pub use errors::{AiError, AiErrorReason, HttpContext, HttpRateLimitDetails, ReasonFields};
pub use events::{
    FinishReason, FinishReasonDetails, LLMEvent, LLMResponse, ResponseState, Usage,
};
pub use ids::{ContentBlockId, ModelId, ProviderId, ProtocolId, ResponseId, RouteId, ToolCallId};
pub use media::{Asset, Kind as MediaKind, Source as MediaSource};
pub use messages::{
    ContentPart, LLMRequest, LLMRequestPatch, Message, MessageRole, SystemPart, ToolChoice,
    ToolDefinition, ToolEntry, ToolNamespace, ToolOutput, ToolResultValue,
};
pub use options::{
    CacheHint, CachePolicy, CachePolicyObject, GenerationOptions, HttpOptions, HttpTimeout,
    LanguageModel, LanguageModelCompatibility, LanguageModelDefaults, ProviderMetadata,
    ProviderOptions, SanitizerCompatibility, DEFAULT_HTTP_TIMEOUT_MS,
};
