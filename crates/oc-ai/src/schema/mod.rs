//! Schema layer: the provider-neutral wire contract (port of
//! `packages/ai/src/schema/`). See module docs on each file for the TS
//! anchor.

pub mod errors;
pub mod events;
pub mod ids;
pub mod media;
pub mod messages;
pub mod options;

pub use errors::ProviderFailureClassification;
pub use errors::{
    AiError, AiErrorReason, HttpContext, HttpRateLimitDetails, ReasonFields, TransportDelivery,
    TransportOperation, TransportPhase, TransportRecovery, TransportType,
    CLASSIFICATION_CONTEXT_OVERFLOW, CLASSIFICATION_INCOMPLETE_STREAM,
    CLASSIFICATION_PAYLOAD_TOO_LARGE,
};
pub use events::{FinishReason, FinishReasonDetails, LLMEvent, LLMResponse, ResponseState, Usage};
pub use ids::{ContentBlockId, ModelId, ProtocolId, ProviderId, ResponseId, RouteId, ToolCallId};
pub use media::{Asset, Kind as MediaKind, Source as MediaSource};
pub use messages::{
    ContentPart, LLMRequest, LLMRequestPatch, Message, MessageRole, SystemPart, ToolChoice,
    ToolContentItems, ToolDefinition, ToolEntry, ToolNamespace, ToolOutput, ToolResultValue,
};
pub use options::{
    CacheHint, CachePolicy, CachePolicyMessages, CachePolicyObject, GenerationOptions, HttpOptions,
    HttpTimeout, LanguageModel, LanguageModelCompatibility, LanguageModelDefaults,
    ProviderMetadata, ProviderOptions, SanitizerCompatibility, DEFAULT_HTTP_TIMEOUT_MS,
};
