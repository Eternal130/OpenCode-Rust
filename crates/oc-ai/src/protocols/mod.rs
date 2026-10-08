//! Protocol implementations and shared utilities.
//!
//! Concrete protocol state machines land in later lanes; the empty modules
//! below are their fixed homes (mirroring `packages/ai/src/protocols/`).

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

// TODO(protocol-lane): provider-neutral Responses API baseline (~8 providers
// build on this) — input items, hosted tool items, function tools, SSE via
// sse framing or WS channel, ProviderAdapter iface per provider.
pub mod open_responses;

// TODO(protocol-lane): OpenAI Responses — OpenResponses body + OpenAI
// extensions (ContextManagement compaction, CompactionTrigger), defaults
// {store:false, include:["reasoning.encrypted_content"]}, hosted tools via
// ToolDefinition.native.openai.
pub mod openai_responses;

// TODO(protocol-lane): openai-compatible re-badging — pure Route::with()
// wrappers reusing OpenAIChat/OpenResponses protocols verbatim; the seam for
// ~25 catalog providers.
pub mod openai_compatible;

// TODO(protocol-lane): Gemini generateContent — URL embeds the model
// (/models/${model}:streamGenerateContent?alt=sse), inline functionCall
// args (object, not string) synthesized into tool-input start/delta/end,
// JSON-schema sanitizer "gemini", onHalt synthesizes finish.
pub mod gemini;
