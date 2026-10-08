//! Protocol implementations and shared utilities.
//!
//! Concrete protocol state machines land in later lanes; the empty modules
//! below are their fixed homes (mirroring `packages/ai/src/protocols/`).

pub mod sanitizer;
pub mod shared;
pub mod utils;

// TODO(protocol-lane): Anthropic Messages API — content blocks, sseEvents
// framing ({message_start, message_delta, message_stop, content_block_*,
// ping, error}), ToolStream accumulation, per-block reasoning signatures,
// auto beta headers, usage merge message_start+message_delta, effort updates.
pub mod anthropic_messages;

// TODO(protocol-lane): OpenAI Chat Completions — sseWithDone framing,
// choices[0].delta, index-keyed tool-call accumulation with partial-json,
// reasoning via compat reasoningField (default reasoning_content) +
// reasoning_details array, finishEvents onHalt synthesizing finish when
// requireFinishReason === false. Provider extensions (deepseek/groq/
// openrouter/venice/zai/alibaba) wrap the body builder only.
pub mod openai_chat;

// Open Responses (provider-neutral baseline, ~8 providers build on this) —
// input items, hosted tool items, function tools, SSE framing, and the
// `ProviderAdapter` seam. WebSocket channel/continuation deferred (ai-map §10).
pub mod open_responses;

// OpenAI Responses — Open Responses + OpenAI extensions (ContextManagement
// compaction, CompactionTrigger), defaults
// {store:false, include:["reasoning.encrypted_content"]}, hosted tools via
// `ToolDefinition.native.openai`. SSE parity path; WS channel deferred.
pub mod openai_responses;

// TODO(protocol-lane): openai-compatible re-badging — pure Route::with()
// wrappers reusing OpenAIChat/OpenResponses protocols verbatim; the seam for
// ~25 catalog providers.
pub mod openai_compatible;

// Gemini generateContent — implemented in `gemini.rs` + the shared
// generate-content utilities and the gemini JSON-Schema projection in
// `utils/`, plus the ToolSchemaProjection in `sanitizer.rs`.
pub mod gemini;

// Cohere v2 chat — native typed SSE events (thinking blocks, tool plans),
// tool accumulation through the shared tool stream; message-end terminal.
pub mod cohere_chat;

// Mistral chat completions — OpenAI-shaped deltas with fragmentary tool
// identity normalized through the Mistral tool-id hasher, native thinking
// blocks, [DONE] terminal over sseWithDone framing.
pub mod mistral_chat;
