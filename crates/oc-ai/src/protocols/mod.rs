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

// Azure OpenAI (provider wiring, ledger AI-AZURE) — URL/api-version/api-key
// construction over the OpenAI protocols (`providers/azure.ts`).
pub mod azure;

// Open Responses (provider-neutral baseline, ~8 providers build on this) —
// input items, hosted tool items, function tools, SSE framing, and the
// `ProviderAdapter` seam. The WebSocket channel/continuation transport
// lives in `open_responses_channel` / `open_responses_continuation`
// (AI-M1); this module carries the shared event decoding.
pub mod open_responses;

// Open Responses channel transport (AI-M1) — the WS form of an Open
// Responses request: create message, base channel driver, HTTP fallback,
// and the stream glue over a WebSocketChannelExecutor.
pub mod open_responses_channel;

// Open Responses continuation (AI-M1) — checkpoint-based incremental sends
// (`previous_response_id` + input suffix) with provider rejection mapping.
pub mod open_responses_continuation;

// OpenAI Responses — Open Responses + OpenAI extensions (ContextManagement
// compaction, CompactionTrigger), defaults
// {store:false, include:["reasoning.encrypted_content"]}, hosted tools via
// `ToolDefinition.native.openai`. SSE parity path; the WS channel transport
// is `channel_transport()` below (AI-M1).
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

// Amazon Bedrock — SigV4 signing, the AWS event-stream binary framing, and
// the Converse protocol (+ anthropic-on-bedrock invoke-stream composition).
pub mod bedrock;

// Google Interactions (AI-M3) — the Gemini Interactions lane:
// thought-signature stateless replay, previous_interaction_id
// continuation, native function_call/function_result steps.
pub mod google_interactions;

// Google Vertex native — the Gemini generateContent protocol on Vertex with
// project/location routing, tuned endpoints, Express Mode keys, ADC bearer.
pub mod vertex;

// xAI Responses (AI-M5) — the Open Responses baseline with the Grok
// hosted tool vocabulary (x_search_call, image_generation_call) and an
// explicit in-band-compaction rejection.
pub mod xai_responses;

// GitHub Copilot chat — the openai-compatible chat protocol with the
// Copilot client header policy over the credential-bound transport.
pub mod copilot;

// GitHub Copilot responses lane (AI-COPILOT-C1) — per-model endpoint
// selection between the `/responses` (OpenAI Responses) and
// `/chat/completions` (openai-compatible chat) deployments, plus the
// dispatching protocol/route that serves both behind one facade binding.
pub mod copilot_responses;
