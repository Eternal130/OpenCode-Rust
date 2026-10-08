//! Plugin hook registry — Rust host semantics for the v2 extension surface
//! (port of `packages/core/src/plugin/hooks.ts` + the `packages/plugin`
//! Context domains, per `.slim/deepwork/rust-port/plugin-map.md` §2).
//!
//! # Binding constraints (oracle gate 2, findings 3 + 6)
//!
//! **Async-first.** Every dispatch/registration surface here is `async`. The
//! only synchronous surface in v2 is the transform/editor domain (editor
//! callbacks are sync even in the Effect API — plugin-map §3), mirrored by
//! [`TransformDomain`]'s `Fn(&mut T)` editors. `oc-plugin-host` implements
//! the blocking bridge internally (block-on with timeout over its RPC); a
//! sync trait here would stall its executors.
//!
//! **Mutation semantics.** v2 hooks mutate live event objects by reference:
//! `trigger(domain, name, event)` walks handlers in **registration order**,
//! handing each the *same* mutable object, then returns it. This registry
//! preserves that for in-process handlers: [`HookRegistry`] dispatch takes
//! the event by value, passes `&mut` through every handler sequentially, and
//! returns the (possibly modified) value.
//!
//! *Documented divergence (gate 2, finding 3):* once events cross a
//! subprocess boundary the semantics become copy-in/copy-out — each guest
//! handler gets its own copy and the modified copy flows back before the
//! next handler. The net effect is identical for well-behaved plugins;
//! pathological plugins that rely on observing mid-call mutation of the
//! host's live object will see a copy instead. The divergence lives at the
//! host bridge, not here: in-process Rust handlers keep true `&mut` chains.
//!
//! # Hook points (the complete v2 surface)
//!
//! | domain | points |
//! |---|---|
//! | session | `prompt`, `context`, `compaction`, `generate`, `title`, `model.request`, `http.request`, `http.response`, `experimental.ws.handshake`, `experimental.ws.send`, `experimental.ws.receive`, `retry` |
//! | tool | `execute.before`, `execute.after` |
//! | permission | `evaluate` |
//! | shell | `create.before` |
//! | aisdk | `sdk`, `language` |
//!
//! Provider scoping: session + aisdk hooks accept
//! [`HookOptions::provider_id`] and are skipped when the event's
//! `model.providerID` doesn't match (v2 `ModelHooks`). Tool/permission/shell
//! events carry no model (provider resolves to `None`), so a scoped
//! registration never fires — identical to v2's `eventProviderID` behavior.
//!
//! The **transform domains** (agent, command, integration, mcp, model,
//! provider, reference, skill, tool-registry, vcs, websearch, worktree) live
//! in [`TransformDomains`]. **Event subscription** (the `event` domain's
//! async iterator) is a [`tokio::sync::broadcast`] channel. The **storage
//! KV** and **RPC** ports are trait surfaces only (`TODO(plugin-lane)`).

// The registration/disposal surface is async-first *by contract* (oracle
// gate 2, finding 6: oc-plugin-host implements the blocking bridge
// internally); the bodies are sync on purpose.
#![allow(
    clippy::unused_async_trait_impl,
    reason = "async-first hook surface per oracle gate 2 finding 6"
)]

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use oc_schema::id::{AgentId, MessageId, SessionId};
use oc_schema::model::Ref as ModelRef;
use oc_schema::permission::Effect as PermissionEffect;
use oc_schema::prompt::Prompt;
use oc_schema::session_inbox::Delivery;
use oc_schema::tool::ToolError;
use parking_lot::Mutex;
use serde_json::Value;

/// JSON object (`Record<string, unknown>`).
pub type JsonRecord = serde_json::Map<String, Value>;

/// Broadcast capacity for the event subscription channel (parity with the
/// SSE feed's 4096-event buffer, core-deep-map §2f).
pub const EVENT_CHANNEL_CAPACITY: usize = 4096;

// =============================================================================
// Registration primitives
// =============================================================================

/// Opaque identifier for one hook/transform registration (diagnostics +
/// identity; disposal goes through [`Registration`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegistrationId(pub u64);

impl fmt::Display for RegistrationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A live registration; disposing removes it (v2 `Registration.dispose()`).
/// Dropping the registration disposes it as well.
pub struct Registration {
    pub id: RegistrationId,
    disposer: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl Registration {
    pub(crate) fn new(id: RegistrationId, disposer: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { id, disposer: Mutex::new(Some(disposer)) }
    }

    /// Dispose the registration (parity: async `dispose(): Promise<void>`).
    /// Idempotent.
    #[allow(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "async surface parity (oracle gate 2 finding 6); removal is sync"
    )]
    pub async fn dispose(self) {
        self.take_disposer();
    }

    /// Synchronous disposal (used by [`HookScope`]'s `Drop`). Idempotent.
    pub fn dispose_sync(&self) {
        self.take_disposer();
    }

    fn take_disposer(&self) {
        if let Some(disposer) = self.disposer.lock().take() {
            disposer();
        }
    }
}

impl fmt::Debug for Registration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Registration({})", self.id)
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(disposer) = self.disposer.lock().take() {
            disposer();
        }
    }
}

/// Options accepted by hook registration (v2 `ModelHookOptions`).
#[derive(Debug, Clone, Default)]
pub struct HookOptions {
    /// Limit the hook to one provider; unscoped hooks apply to every
    /// provider. Only consulted for events carrying a model.
    pub provider_id: Option<String>,
}

/// Hook failure channel. v2 types every domain except `tool.execute.before`
/// as infallible (`NoFailures`); a defecting handler fails the dispatch.
/// `execute.before` alone carries a typed failure: a `Tool.Error` rejecting
/// the call before it runs.
#[derive(Debug, Clone)]
pub enum HookError {
    /// `tool.execute.before` rejection — the tool call fails before running.
    ToolRejected(ToolError),
    /// Handler failed (guest exception surfaced by the host bridge, or a
    /// native handler error). Remaining handlers are skipped.
    Handler(String),
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HookError::ToolRejected(error) => {
                write!(f, "tool execution rejected by hook: {}", error.message)
            }
            HookError::Handler(message) => write!(f, "hook handler failed: {message}"),
        }
    }
}

impl std::error::Error for HookError {}

impl From<String> for HookError {
    fn from(message: String) -> Self {
        HookError::Handler(message)
    }
}

/// Async handler for one hook point (the seam `oc-plugin-host` implements
/// over its RPC; native handlers use [`fn_hook`] or implement this directly).
#[async_trait]
pub trait HookHandler<E>: Send + Sync {
    /// Observe and possibly mutate the event. Sequential; called in
    /// registration order.
    async fn call(&self, event: &mut E) -> Result<(), HookError>;
}

/// Wrap a synchronous closure as a [`HookHandler`].
pub struct FnHook<F>(pub F);

#[async_trait]
impl<E, F> HookHandler<E> for FnHook<F>
where
    E: Send + 'static,
    F: Fn(&mut E) -> Result<(), HookError> + Send + Sync,
{
    async fn call(&self, event: &mut E) -> Result<(), HookError> {
        (self.0)(event)
    }
}

/// Convenience constructor wrapping a sync closure as a shared handler.
pub fn fn_hook<E, F>(f: F) -> Arc<dyn HookHandler<E>>
where
    E: Send + 'static,
    F: Fn(&mut E) -> Result<(), HookError> + Send + Sync + 'static,
{
    Arc::new(FnHook(f))
}

/// Events carrying a `model.providerID` participate in provider scoping
/// (session + aisdk hooks, v2 `eventProviderID`); the rest resolve to
/// `None` so a scoped registration never fires for them.
pub trait HookEventProvider {
    fn hook_provider(&self) -> Option<String>;
}

impl HookEventProvider for SessionPromptEvent {
    fn hook_provider(&self) -> Option<String> {
        None
    }
}

impl HookEventProvider for SessionContextEvent {
    fn hook_provider(&self) -> Option<String> {
        Some(self.model.provider_id.0.clone())
    }
}

impl HookEventProvider for SessionCompactionEvent {
    fn hook_provider(&self) -> Option<String> {
        Some(self.context.model.provider_id.0.clone())
    }
}

impl HookEventProvider for SessionTitleEvent {
    fn hook_provider(&self) -> Option<String> {
        Some(self.request.model.provider_id.0.clone())
    }
}

impl HookEventProvider for SessionModelRequestEvent {
    fn hook_provider(&self) -> Option<String> {
        Some(self.model.provider_id.0.clone())
    }
}

impl HookEventProvider for SessionHttpRequestEvent {
    fn hook_provider(&self) -> Option<String> {
        Some(self.model.provider_id.0.clone())
    }
}

impl HookEventProvider for SessionHttpResponseEvent {
    fn hook_provider(&self) -> Option<String> {
        Some(self.model.provider_id.0.clone())
    }
}

impl HookEventProvider for SessionWsHandshakeEvent {
    fn hook_provider(&self) -> Option<String> {
        Some(self.model.provider_id.0.clone())
    }
}

impl HookEventProvider for SessionWsSendEvent {
    fn hook_provider(&self) -> Option<String> {
        Some(self.model.provider_id.0.clone())
    }
}

impl HookEventProvider for SessionWsReceiveEvent {
    fn hook_provider(&self) -> Option<String> {
        Some(self.model.provider_id.0.clone())
    }
}

impl HookEventProvider for SessionRetryEvent {
    fn hook_provider(&self) -> Option<String> {
        Some(self.model.provider_id.0.clone())
    }
}

impl HookEventProvider for ToolExecuteBeforeEvent {
    fn hook_provider(&self) -> Option<String> {
        None
    }
}

impl HookEventProvider for ToolExecuteAfterEvent {
    fn hook_provider(&self) -> Option<String> {
        None
    }
}

impl HookEventProvider for PermissionEvaluateEvent {
    fn hook_provider(&self) -> Option<String> {
        None
    }
}

impl HookEventProvider for ShellCreateBeforeEvent {
    fn hook_provider(&self) -> Option<String> {
        None
    }
}

impl HookEventProvider for AisdkSdkEvent {
    fn hook_provider(&self) -> Option<String> {
        Some(self.model.provider_id.0.clone())
    }
}

impl HookEventProvider for AisdkLanguageEvent {
    fn hook_provider(&self) -> Option<String> {
        Some(self.model.provider_id.0.clone())
    }
}

struct HookEntry<E> {
    id: RegistrationId,
    provider_filter: Option<String>,
    handler: Arc<dyn HookHandler<E>>,
}

struct HookList<E> {
    entries: Vec<HookEntry<E>>,
}

impl<E> Default for HookList<E> {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
}

// =============================================================================
// Event payloads (Rust-side contracts of the v2 hook shapes)
// =============================================================================
// Identity fields are read-only (v2 `readonly`); mutable fields are the ones
// v2 plugins mutate. `oc-plugin-host` bridges guest JS objects to these.

/// `session.prompt` — prompt admission with a mutable prompt.
#[derive(Debug, Clone)]
pub struct SessionPromptEvent {
    pub session_id: SessionId,
    pub message_id: MessageId,
    pub prompt: Prompt,
    pub metadata: Option<JsonRecord>,
    pub delivery: Delivery,
}

/// Request options: typed generation settings + arbitrary provider options
/// (`GenerationOptionsFields & Record<string, unknown>`), kept as one JSON
/// record.
pub type RequestOptions = JsonRecord;

/// `session.title` / `session.generate` request base.
#[derive(Debug, Clone)]
pub struct SessionRequestEvent {
    pub session_id: SessionId,
    pub model: ModelRef,
    pub system: Vec<oc_ai::schema::messages::SystemPart>,
    pub messages: Vec<oc_ai::schema::messages::Message>,
    pub options: RequestOptions,
}

/// Tool descriptor exposed on context events (`tools` map).
#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    pub description: String,
    pub input: Value,
}

/// `session.context` — pre-dispatch context (system parts, messages,
/// options, tools).
#[derive(Debug, Clone)]
pub struct SessionContextEvent {
    pub session_id: SessionId,
    pub agent: AgentId,
    pub model: ModelRef,
    pub system: Vec<oc_ai::schema::messages::SystemPart>,
    pub messages: Vec<oc_ai::schema::messages::Message>,
    pub options: RequestOptions,
    pub tools: BTreeMap<String, ToolDescriptor>,
}

/// Compaction result short-circuit (`SessionCompaction.result`).
#[derive(Debug, Clone, Default)]
pub struct CompactionResult {
    pub summary: String,
    pub provider_state: Option<Value>,
    pub metadata: Option<JsonRecord>,
    pub tokens: Option<Value>,
}

/// `session.compaction` — context + optional short-circuit result.
#[derive(Debug, Clone)]
pub struct SessionCompactionEvent {
    pub context: SessionContextEvent,
    /// Set to use this compaction and skip the model request.
    pub result: Option<CompactionResult>,
}

/// `session.generate` — same shape as context.
pub type SessionGenerateEvent = SessionContextEvent;

/// `session.title` — request + optional short-circuit title.
#[derive(Debug, Clone)]
pub struct SessionTitleEvent {
    pub request: SessionRequestEvent,
    /// Set to use this title and skip the model request.
    pub result: Option<String>,
}

/// Why a session request is being made (auxiliary requests share the
/// session's hook identity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRequestKind {
    Primary,
    Compaction,
    Title,
    Generate,
}

impl SessionRequestKind {
    /// Wire name (v2 `SessionRequestKind`).
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionRequestKind::Primary => "primary",
            SessionRequestKind::Compaction => "compaction",
            SessionRequestKind::Title => "title",
            SessionRequestKind::Generate => "generate",
        }
    }
}

/// `session.model.request` — per-request route knobs.
#[derive(Debug, Clone)]
pub struct SessionModelRequestEvent {
    pub session_id: SessionId,
    pub agent: AgentId,
    pub model: ModelRef,
    pub kind: SessionRequestKind,
    pub base_url: Option<String>,
    pub headers: BTreeMap<String, String>,
}

/// Mutable HTTP request shape (v2 carries a `Request`; the Rust contract is
/// the plain parts plugins actually rewrite).
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub url: String,
    pub method: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

/// Mutable HTTP response shape.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

/// `session.http.request`.
#[derive(Debug, Clone)]
pub struct SessionHttpRequestEvent {
    pub session_id: SessionId,
    pub agent: AgentId,
    pub model: ModelRef,
    pub kind: SessionRequestKind,
    pub request: HttpRequest,
}

/// `session.http.response`.
#[derive(Debug, Clone)]
pub struct SessionHttpResponseEvent {
    pub session_id: SessionId,
    pub agent: AgentId,
    pub model: ModelRef,
    pub kind: SessionRequestKind,
    pub request: HttpRequest,
    pub response: HttpResponse,
}

/// `session.experimental.ws.handshake` — socket selection (experimental).
#[derive(Debug, Clone)]
pub struct SessionWsHandshakeEvent {
    pub session_id: SessionId,
    pub agent: AgentId,
    pub model: ModelRef,
    pub kind: SessionRequestKind,
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

/// `session.experimental.ws.send` — outbound frame rewrite (experimental).
#[derive(Debug, Clone)]
pub struct SessionWsSendEvent {
    pub session_id: SessionId,
    pub agent: AgentId,
    pub model: ModelRef,
    pub kind: SessionRequestKind,
    pub frame: String,
}

/// `session.experimental.ws.receive` — inbound frame rewrite (experimental).
#[derive(Debug, Clone)]
pub struct SessionWsReceiveEvent {
    pub session_id: SessionId,
    pub agent: AgentId,
    pub model: ModelRef,
    pub kind: SessionRequestKind,
    pub frame: String,
}

/// Retry decision: `{retry:false}` or `{retry:true, delay}`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RetryDecision {
    NoRetry,
    Retry { delay_ms: u64 },
}

/// `session.retry`.
#[derive(Debug, Clone)]
pub struct SessionRetryEvent {
    pub session_id: SessionId,
    pub agent: AgentId,
    pub model: ModelRef,
    pub error: Value,
    pub attempt: u32,
    pub decision: RetryDecision,
}

/// `tool.execute.before`.
#[derive(Debug, Clone)]
pub struct ToolExecuteBeforeEvent {
    pub tool: String,
    pub session_id: SessionId,
    pub agent: AgentId,
    pub message_id: MessageId,
    pub call_id: String,
    pub input: Value,
}

/// Completed-vs-error outcome of `tool.execute.after` (mutable `result` /
/// `error` legs).
#[derive(Debug, Clone)]
pub enum ToolOutcome {
    Completed { result: Value },
    Error { error: ToolError },
}

/// `tool.execute.after`.
#[derive(Debug, Clone)]
pub struct ToolExecuteAfterEvent {
    pub tool: String,
    pub session_id: SessionId,
    pub agent: AgentId,
    pub message_id: MessageId,
    pub call_id: String,
    pub input: Value,
    pub outcome: ToolOutcome,
}

/// `permission.evaluate`.
#[derive(Debug, Clone)]
pub struct PermissionEvaluateEvent {
    pub session_id: SessionId,
    pub agent: Option<AgentId>,
    pub action: String,
    pub resources: Vec<String>,
    pub metadata: Option<JsonRecord>,
    pub source: Option<Value>,
    pub effect: PermissionEffect,
    pub message: Option<String>,
}

/// `shell.create.before`.
#[derive(Debug, Clone)]
pub struct ShellCreateBeforeEvent {
    pub command: String,
    pub cwd: String,
    pub timeout_ms: u64,
    pub shell: String,
    pub env: BTreeMap<String, Option<String>>,
}

/// Opaque AI-SDK package instance handle (`sdk?: any`).
pub type SdkHandle = Arc<dyn Any + Send + Sync>;

/// `aisdk.sdk` — swap the AI-SDK package instance.
#[derive(Debug, Clone)]
pub struct AisdkSdkEvent {
    pub model: oc_schema::model::Info,
    pub package: String,
    pub options: JsonRecord,
    pub sdk: Option<SdkHandle>,
}

/// `aisdk.language` — swap the `LanguageModel` implementation (v2
/// `LanguageModelV3`; the Rust equivalent is the fully-configured
/// [`oc_ai::schema::options::LanguageModel`]).
#[derive(Clone)]
pub struct AisdkLanguageEvent {
    pub model: oc_schema::model::Info,
    pub sdk: Option<SdkHandle>,
    pub options: JsonRecord,
    pub language: Option<oc_ai::LanguageModel>,
}

impl fmt::Debug for AisdkLanguageEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AisdkLanguageEvent")
            .field("model", &self.model.id)
            .field("has_sdk", &self.sdk.is_some())
            .field("language", &self.language.as_ref().map(|model| model.id.0.clone()))
            .finish_non_exhaustive()
    }
}

// =============================================================================
// The registry
// =============================================================================

/// Generates the 18 typed hook fields + register/dispatch/has methods.
/// Provider scoping comes from [`HookEventProvider`] (v2
/// `eventProviderID`).
macro_rules! oc_hook_points {
    ($( $field:ident, $register:ident, $dispatch:ident, $has:ident, $point:ident,
        $event:ty, $key:literal );* $(;)?) => {
        /// Every v2 hook point, as an enum (diagnostics, `has()` probes).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum HookPoint {
            $( $point, )*
        }

        impl HookPoint {
            /// All points, in registry declaration order.
            pub fn all() -> &'static [HookPoint] {
                &[ $( HookPoint::$point, )* ]
            }

            /// v2 `domain.name` wire key.
            #[must_use]
            pub fn key(&self) -> &'static str {
                match self {
                    $( HookPoint::$point => $key, )*
                }
            }
        }

        struct HookLists {
            $( $field: Mutex<HookList<$event>>, )*
        }

        impl HookRegistry {
            $(
                #[doc = concat!("Register a `", $key, "` handler (async surface; oracle gate 2 finding 6).")]
                #[allow(
                    clippy::unused_async,
        clippy::unused_async_trait_impl,
                    reason = "async-first registration surface; storage is sync"
                )]
                pub async fn $register(
                    &self,
                    handler: Arc<dyn HookHandler<$event>>,
                    options: HookOptions,
                ) -> Registration {
                    let id = RegistrationId(self.next_id.fetch_add(1, Ordering::SeqCst));
                    self.lists.$field.lock().entries.push(HookEntry {
                        id,
                        provider_filter: options.provider_id,
                        handler,
                    });
                    let lists = Arc::clone(&self.lists);
                    Registration::new(id, Arc::new(move || {
                        lists.$field.lock().entries.retain(|entry| entry.id != id);
                    }))
                }

                #[doc = concat!("Dispatch `", $key, "`: sequential in registration order, `&mut` event chaining, returns the final value.")]
                pub async fn $dispatch(&self, mut event: $event) -> Result<$event, HookError> {
                    let provider = event.hook_provider();
                    let handlers = {
                        let guard = self.lists.$field.lock();
                        guard
                            .entries
                            .iter()
                            .map(|entry| {
                                (entry.provider_filter.clone(), Arc::clone(&entry.handler))
                            })
                            .collect::<Vec<_>>()
                    };
                    for (filter, handler) in handlers {
                        if let Some(filter) = filter.as_deref() {
                            if provider.as_deref() != Some(filter) {
                                continue;
                            }
                        }
                        handler.call(&mut event).await?;
                    }
                    Ok(event)
                }

                #[doc = concat!("Whether `", $key, "` has a matching registration (v2 `has`).")]
                #[must_use]
                pub fn $has(&self, provider_id: Option<&str>) -> bool {
                    self.lists.$field.lock().entries.iter().any(|entry| {
                        match entry.provider_filter.as_deref() {
                            None => true,
                            Some(filter) => provider_id == Some(filter),
                        }
                    })
                }
            )*
        }
    };
}

/// The per-location plugin hook registry (v2 `PluginHooks` service, one
/// instance per `LocationGraph`).
pub struct HookRegistry {
    next_id: Arc<AtomicU64>,
    lists: Arc<HookLists>,
    transforms: TransformDomains,
    events: tokio::sync::broadcast::Sender<EventEnvelope>,
}

impl fmt::Debug for HookRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookRegistry")
            .field("registrations", &self.total_hooks())
            .finish_non_exhaustive()
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HookRegistry {
    /// Fresh registry with no registrations.
    #[must_use]
    pub fn new() -> Self {
        let (events, _) = tokio::sync::broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            next_id: Arc::new(AtomicU64::new(1)),
            lists: Arc::new(HookLists {
                session_prompt: Mutex::new(HookList::default()),
                session_context: Mutex::new(HookList::default()),
                session_compaction: Mutex::new(HookList::default()),
                session_generate: Mutex::new(HookList::default()),
                session_title: Mutex::new(HookList::default()),
                session_model_request: Mutex::new(HookList::default()),
                session_http_request: Mutex::new(HookList::default()),
                session_http_response: Mutex::new(HookList::default()),
                session_ws_handshake: Mutex::new(HookList::default()),
                session_ws_send: Mutex::new(HookList::default()),
                session_ws_receive: Mutex::new(HookList::default()),
                session_retry: Mutex::new(HookList::default()),
                tool_execute_before: Mutex::new(HookList::default()),
                tool_execute_after: Mutex::new(HookList::default()),
                permission_evaluate: Mutex::new(HookList::default()),
                shell_create_before: Mutex::new(HookList::default()),
                aisdk_sdk: Mutex::new(HookList::default()),
                aisdk_language: Mutex::new(HookList::default()),
            }),
            transforms: TransformDomains::new(),
            events,
        }
    }

    /// The transform domains owned by this registry (sharing its id space).
    #[must_use]
    pub fn transforms(&self) -> &TransformDomains {
        &self.transforms
    }

    /// Count of live registrations across every hook point (diagnostics).
    #[must_use]
    pub fn total_hooks(&self) -> usize {
        let lists = &*self.lists;
        lists.session_prompt.lock().entries.len()
            + lists.session_context.lock().entries.len()
            + lists.session_compaction.lock().entries.len()
            + lists.session_generate.lock().entries.len()
            + lists.session_title.lock().entries.len()
            + lists.session_model_request.lock().entries.len()
            + lists.session_http_request.lock().entries.len()
            + lists.session_http_response.lock().entries.len()
            + lists.session_ws_handshake.lock().entries.len()
            + lists.session_ws_send.lock().entries.len()
            + lists.session_ws_receive.lock().entries.len()
            + lists.session_retry.lock().entries.len()
            + lists.tool_execute_before.lock().entries.len()
            + lists.tool_execute_after.lock().entries.len()
            + lists.permission_evaluate.lock().entries.len()
            + lists.shell_create_before.lock().entries.len()
            + lists.aisdk_sdk.lock().entries.len()
            + lists.aisdk_language.lock().entries.len()
    }

    // -- event subscription (the `event` domain) ------------------------------

    /// Broadcast a server event to subscribers (the bus lane taps this;
    /// `TODO(lane-integration)`).
    #[must_use]
    pub fn publish_event(&self, event: EventEnvelope) -> usize {
        self.events.send(event).unwrap_or(0)
    }

    /// Subscribe to the live event stream (v2 `event.subscribe` async
    /// iterator). Late subscribers see only future events.
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<EventEnvelope> {
        self.events.subscribe()
    }

    /// Subscribe as a [`futures::Stream`].
    ///
    /// Overflow divergence: v2's SSE feed *terminates* a lagging subscriber
    /// (`SubscriberOverflowError`, drop-and-disconnect); the broadcast
    /// channel keeps reading after a `Lagged` gap instead. The server lane's
    /// SSE adapter owns v2's termination behavior.
    pub fn subscribe_stream(&self) -> impl futures::Stream<Item = EventEnvelope> {
        let receiver = self.subscribe();
        futures::stream::unfold(receiver, |mut receiver| async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => return Some((event, receiver)),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        })
    }
}

oc_hook_points! {
    session_prompt, register_session_prompt, dispatch_session_prompt, has_session_prompt,
        SessionPrompt, SessionPromptEvent, "session.prompt";
    session_context, register_session_context, dispatch_session_context, has_session_context,
        SessionContext, SessionContextEvent, "session.context";
    session_compaction, register_session_compaction, dispatch_session_compaction, has_session_compaction,
        SessionCompaction, SessionCompactionEvent, "session.compaction";
    session_generate, register_session_generate, dispatch_session_generate, has_session_generate,
        SessionGenerate, SessionGenerateEvent, "session.generate";
    session_title, register_session_title, dispatch_session_title, has_session_title,
        SessionTitle, SessionTitleEvent, "session.title";
    session_model_request, register_session_model_request, dispatch_session_model_request, has_session_model_request,
        SessionModelRequest, SessionModelRequestEvent, "session.model.request";
    session_http_request, register_session_http_request, dispatch_session_http_request, has_session_http_request,
        SessionHttpRequest, SessionHttpRequestEvent, "session.http.request";
    session_http_response, register_session_http_response, dispatch_session_http_response, has_session_http_response,
        SessionHttpResponse, SessionHttpResponseEvent, "session.http.response";
    session_ws_handshake, register_session_ws_handshake, dispatch_session_ws_handshake, has_session_ws_handshake,
        SessionWsHandshake, SessionWsHandshakeEvent, "session.experimental.ws.handshake";
    session_ws_send, register_session_ws_send, dispatch_session_ws_send, has_session_ws_send,
        SessionWsSend, SessionWsSendEvent, "session.experimental.ws.send";
    session_ws_receive, register_session_ws_receive, dispatch_session_ws_receive, has_session_ws_receive,
        SessionWsReceive, SessionWsReceiveEvent, "session.experimental.ws.receive";
    session_retry, register_session_retry, dispatch_session_retry, has_session_retry,
        SessionRetry, SessionRetryEvent, "session.retry";
    tool_execute_before, register_tool_execute_before, dispatch_tool_execute_before, has_tool_execute_before,
        ToolExecuteBefore, ToolExecuteBeforeEvent, "tool.execute.before";
    tool_execute_after, register_tool_execute_after, dispatch_tool_execute_after, has_tool_execute_after,
        ToolExecuteAfter, ToolExecuteAfterEvent, "tool.execute.after";
    permission_evaluate, register_permission_evaluate, dispatch_permission_evaluate, has_permission_evaluate,
        PermissionEvaluate, PermissionEvaluateEvent, "permission.evaluate";
    shell_create_before, register_shell_create_before, dispatch_shell_create_before, has_shell_create_before,
        ShellCreateBefore, ShellCreateBeforeEvent, "shell.create.before";
    aisdk_sdk, register_aisdk_sdk, dispatch_aisdk_sdk, has_aisdk_sdk,
        AisdkSdk, AisdkSdkEvent, "aisdk.sdk";
    aisdk_language, register_aisdk_language, dispatch_aisdk_language, has_aisdk_language,
        AisdkLanguage, AisdkLanguageEvent, "aisdk.language";
}

// =============================================================================
// Scoped cleanup
// =============================================================================

/// A scope collecting registrations for scoped cleanup (v2 `Effect.Scope` +
/// `Scope.addFinalizer`): dropping the scope disposes everything tracked.
#[derive(Default)]
pub struct HookScope {
    registrations: Mutex<Vec<Registration>>,
}

impl fmt::Debug for HookScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HookScope({} registrations)", self.registrations.lock().len())
    }
}

impl HookScope {
    /// Empty scope.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Track a registration; it is disposed when the scope drops or closes.
    pub fn track(&self, registration: Registration) {
        self.registrations.lock().push(registration);
    }

    /// Number of live tracked registrations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.registrations.lock().len()
    }

    /// Whether the scope tracks nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.registrations.lock().is_empty()
    }

    /// Explicitly dispose everything (parity with v2's awaited scope close);
    /// afterwards the scope is empty.
    #[allow(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "async surface parity; disposal itself is sync"
    )]
    pub async fn close(&self) {
        for registration in self.registrations.lock().drain(..) {
            registration.dispose_sync();
        }
    }
}

impl Drop for HookScope {
    fn drop(&mut self) {
        for registration in self.registrations.get_mut().drain(..) {
            registration.dispose_sync();
        }
    }
}

// =============================================================================
// Transform domains
// =============================================================================

/// The v2 transform domains (`ctx.agent`, `ctx.command`, …). State types are
/// supplied by the owning lanes via [`DomainState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DomainId {
    Agent,
    Command,
    Integration,
    Mcp,
    Model,
    Provider,
    Reference,
    Skill,
    ToolRegistry,
    Vcs,
    Websearch,
    Worktree,
}

impl DomainId {
    /// v2 context property name.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            DomainId::Agent => "agent",
            DomainId::Command => "command",
            DomainId::Integration => "integration",
            DomainId::Mcp => "mcp",
            DomainId::Model => "model",
            DomainId::Provider => "provider",
            DomainId::Reference => "reference",
            DomainId::Skill => "skill",
            DomainId::ToolRegistry => "tool",
            DomainId::Vcs => "vcs",
            DomainId::Websearch => "websearch",
            DomainId::Worktree => "worktree",
        }
    }
}

/// State carried by a transform domain (agent list, model table, …). The
/// default is the empty base; the owning lane installs a discovered base via
/// [`TransformDomain::reload_with_base`].
///
/// `TODO(lane-integration)`: the config/agent/skill lanes implement this for
/// their concrete state types.
pub trait DomainState: Clone + Default + Send + Sync + 'static {}

impl<T> DomainState for T where T: Clone + Default + Send + Sync + 'static {}

/// Sync editor callback (`TransformCallback<Editor>` — sync in both v2 APIs).
pub type EditorFn<T> = Box<dyn Fn(&mut T) + Send + Sync>;

struct TransformEntry<T> {
    id: RegistrationId,
    editor: EditorFn<T>,
}

struct TransformInner<T> {
    base: T,
    /// Current value: `base` with every registered editor applied in order,
    /// plus any unregistered `edit()` writes since the last reload.
    snapshot: T,
    edits: Vec<TransformEntry<T>>,
    version: u64,
}

/// One transformable domain (v2 `State.Transformable`: `transform` +
/// `reload`).
///
/// Semantics (plugin-map §2 + `state.ts`):
/// - [`Self::register`] appends a **sync editor** that applies immediately
///   and re-applies on every rebuild (registration order); the returned
///   [`Registration`] removes it and invalidates on dispose.
/// - [`Self::edit`] is read-for-edit: callers fetch the current state,
///   apply a sync editor fn, write back. Unregistered edits persist only
///   until the next reload.
/// - [`Self::reload`] invalidates: re-derives `base ⊕ editors` in order.
///   [`Self::reload_with_base`] also replaces the base (the config/plugin
///   hot path re-runs discovery, then reloads).
/// - [`Self::version`] is the invalidation marker (bumped by every change).
pub struct TransformDomain<T> {
    inner: Mutex<TransformInner<T>>,
    next_id: Arc<AtomicU64>,
}

impl<T> fmt::Debug for TransformDomain<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let guard = self.inner.lock();
        write!(f, "TransformDomain(version={}, editors={})", guard.version, guard.edits.len())
    }
}

impl<T: DomainState> TransformDomain<T> {
    pub(crate) fn new(next_id: Arc<AtomicU64>) -> Self {
        Self {
            inner: Mutex::new(TransformInner {
                base: T::default(),
                snapshot: T::default(),
                edits: Vec::new(),
                version: 0,
            }),
            next_id,
        }
    }

    /// Current derived state (clone of the snapshot).
    #[must_use]
    pub fn state(&self) -> T {
        self.inner.lock().snapshot.clone()
    }

    /// Invalidation marker; changes whenever the state or editor set changed.
    pub fn version(&self) -> u64 {
        self.inner.lock().version
    }

    /// Register a replayable editor (v2 `transform(cb)`). Async surface
    /// (v2 returns `Promise<Registration>`); the editor itself is sync.
    #[allow(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "async-first registration surface (v2 Transform returns a Promise)"
    )]
    pub async fn register(self: &Arc<Self>, editor: EditorFn<T>) -> Registration {
        let id = RegistrationId(self.next_id.fetch_add(1, Ordering::SeqCst));
        {
            let mut guard = self.inner.lock();
            (editor)(&mut guard.snapshot);
            guard.edits.push(TransformEntry { id, editor });
            guard.version += 1;
        }
        let domain = Arc::clone(self);
        Registration::new(id, Arc::new(move || domain.remove_editor(id)))
    }

    /// One-shot read-for-edit mutation (not replayed on reload).
    pub fn edit(&self, editor: impl FnOnce(&mut T)) -> u64 {
        let mut guard = self.inner.lock();
        editor(&mut guard.snapshot);
        guard.version += 1;
        guard.version
    }

    /// Invalidate + re-derive from the current base (v2 `reload()`).
    pub fn reload(&self) -> u64 {
        let mut guard = self.inner.lock();
        Self::rederive(&mut guard);
        guard.version += 1;
        guard.version
    }

    /// Replace the discovered base and re-derive (config/plugin hot path).
    pub fn reload_with_base(&self, base: T) -> u64 {
        let mut guard = self.inner.lock();
        guard.base = base;
        Self::rederive(&mut guard);
        guard.version += 1;
        guard.version
    }

    fn remove_editor(&self, id: RegistrationId) {
        let mut guard = self.inner.lock();
        let before = guard.edits.len();
        guard.edits.retain(|entry| entry.id != id);
        if guard.edits.len() != before {
            Self::rederive(&mut guard);
            guard.version += 1;
        }
    }

    fn rederive(inner: &mut TransformInner<T>) {
        let mut next = inner.base.clone();
        for entry in &inner.edits {
            (entry.editor)(&mut next);
        }
        inner.snapshot = next;
    }
}

/// Type-erased handle over one [`TransformDomain`] for [`TransformDomains`].
trait DomainStore: Send + Sync {
    fn reload(&self);
    fn domain_arc(&self) -> Arc<dyn Any + Send + Sync>;
}

struct TypedDomainStore<T: DomainState> {
    domain: Arc<TransformDomain<T>>,
}

impl<T: DomainState> DomainStore for TypedDomainStore<T> {
    fn reload(&self) {
        self.domain.reload();
    }

    fn domain_arc(&self) -> Arc<dyn Any + Send + Sync> {
        Arc::clone(&self.domain) as Arc<dyn Any + Send + Sync>
    }
}

/// The 12 v2 transform domains, keyed by [`DomainId`] with typed retrieval.
#[derive(Default)]
pub struct TransformDomains {
    stores: DashMap<DomainId, Box<dyn DomainStore>>,
    next_id: Arc<AtomicU64>,
}

impl fmt::Debug for TransformDomains {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let keys: Vec<DomainId> = self.stores.iter().map(|entry| *entry.key()).collect();
        write!(f, "TransformDomains({keys:?})")
    }
}

impl TransformDomains {
    /// Fresh domain set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get (lazily creating) the typed domain for `id`.
    #[must_use]
    pub fn domain<T: DomainState>(&self, id: DomainId) -> Arc<TransformDomain<T>> {
        let entry = self.stores.entry(id).or_insert_with(|| {
            Box::new(TypedDomainStore::<T> {
                domain: Arc::new(TransformDomain::<T>::new(Arc::clone(&self.next_id))),
            })
        });
        entry.domain_arc().downcast::<TransformDomain<T>>().unwrap_or_else(|_| {
            unreachable!("typed domain storage mismatch — state type changed mid-process")
        })
    }

    /// Register a replayable editor on a domain (convenience over
    /// [`TransformDomain::register`]).
    pub async fn register_transform<T: DomainState>(
        &self,
        id: DomainId,
        editor: EditorFn<T>,
    ) -> Registration {
        self.domain::<T>(id).register(editor).await
    }

    /// Reload every materialized domain (config/plugin hot path):
    /// re-derives `base ⊕ editors` per domain, in registration order.
    pub fn reload_all(&self) {
        for entry in &self.stores {
            entry.value().reload();
        }
    }
}

// =============================================================================
// Event envelope (the `event` domain payload)
// =============================================================================

/// Server event envelope (v2 `{id, created, type, location?, data}`).
#[derive(Debug, Clone)]
pub struct EventEnvelope {
    pub id: String,
    pub created: i64,
    pub r#type: String,
    pub location: Option<oc_schema::location::Ref>,
    pub data: Value,
}

// =============================================================================
// Storage KV port (trait surface only)
// =============================================================================

/// Storage scan options (v2 `StorageScanOptions`).
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    pub prefix: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

/// Storage scan page (v2 `StorageScanResult`).
#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    pub items: Vec<(String, Value)>,
    pub cursor: Option<String>,
}

/// Storage backend error.
#[derive(Debug, Clone, thiserror::Error)]
#[error("plugin storage: {0}")]
pub struct StorageError(pub String);

/// Plugin-ID-scoped durable KV (v2 `ctx.storage`). The plugin lane provides
/// the concrete backend (durable table or file store).
///
/// `TODO(plugin-lane)`: implement over the database KV table with plugin-ID
/// key scoping.
#[async_trait]
pub trait PluginStorage: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Value>, StorageError>;
    async fn set(&self, key: &str, value: Value) -> Result<(), StorageError>;
    async fn remove(&self, key: &str) -> Result<(), StorageError>;
    async fn scan(&self, options: ScanOptions) -> Result<ScanResult, StorageError>;
}

// =============================================================================
// RPC port (trait surface only)
// =============================================================================

/// One RPC method's I/O schemas (JSON-Schema-equivalent definitions at the
/// JS boundary; plugin-map §3).
#[derive(Debug, Clone)]
pub struct RpcMethodSchema {
    pub input: Value,
    pub output: Value,
}

/// A portable RPC definition (v2 `Rpc.PortableDefinition`).
#[derive(Debug, Clone)]
pub struct RpcDefinition {
    pub id: String,
    pub methods: BTreeMap<String, RpcMethodSchema>,
    pub events: Vec<String>,
}

/// RPC failure.
#[derive(Debug, Clone, thiserror::Error)]
#[error("rpc: {0}")]
pub struct RpcError(pub String);

/// Dispatches method calls for one registered definition.
#[async_trait]
pub trait RpcHandler: Send + Sync {
    async fn call(&self, method: &str, input: Value) -> Result<Value, RpcError>;
}

/// The RPC port (`ctx.rpc.register` + `events.emit`; callable from clients
/// via `/api/rpc/{rpcID}/{method}`). Trait surface only.
///
/// `TODO(plugin-lane)`: wire to the server router + `rpc.*` events.
#[async_trait]
pub trait RpcPort: Send + Sync {
    async fn register(
        &self,
        definition: RpcDefinition,
        handler: Arc<dyn RpcHandler>,
    ) -> Result<Registration, RpcError>;
    async fn emit(&self, rpc_id: &str, event: &str, data: Value) -> Result<(), RpcError>;
    /// Close the port (location shutdown sequence step).
    async fn close(&self);
}

// =============================================================================
// Internal plugin slots + supervisor (graph v2, oracle gate 4 finding 4)
// =============================================================================

/// Internal-plugin registration failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("plugin {id} failed to register: {error}")]
pub struct PluginFailure {
    pub id: String,
    pub error: String,
}

/// One internal plugin's boot outcome (contained failures; boot continues).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginBootOutcome {
    /// Ids that registered successfully, in boot order.
    pub registered: Vec<String>,
    /// Contained failures (registration errors + duplicate ids).
    pub failures: Vec<PluginFailure>,
}

/// Whether `id` is reserved for internal plugins. Every id under the
/// `"opencode."` namespace is internal (v2 `PluginInternal.guarded`
/// protects the built-ins from removal, `internal.ts:271`; the Rust port
/// reserves the whole namespace so a host plugin can never shadow or
/// disable a built-in).
#[must_use]
pub fn is_reserved_plugin_id(id: &str) -> bool {
    id.starts_with("opencode.")
}

/// A built-in plugin: an id plus a registration closure run at location
/// boot with the fresh [`HookRegistry`] and the graph's
/// [`LocationPorts`](crate::location::LocationPorts) — the Rust shape of
/// v2 `InternalPlugin { id, effect }` (`internal.ts`). The 14 tools, the
/// agent seeds, config watchers, … hand-register through these slots;
/// `oc-plugin-host` plugs guest plugins into the supervisor's host slot
/// later (`TODO(plugin-lane)`).
pub struct InternalPlugin {
    id: String,
    #[allow(clippy::type_complexity)]
    register: Arc<
        dyn for<'a> Fn(&HookRegistry, &crate::location::LocationPorts<'a>) -> Result<(), String>
            + Send
            + Sync,
    >,
}

impl fmt::Debug for InternalPlugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InternalPlugin({})", self.id)
    }
}

impl InternalPlugin {
    /// A plugin registering through `register`. Registration is a sync
    /// closure: v2 internal plugins only touch registries (hook lists,
    /// transform domains, tool tables), and the host lane implements its
    /// own blocking bridge for guest effects (see the module docs).
    pub fn new(
        id: impl Into<String>,
        register: impl for<'a> Fn(&HookRegistry, &crate::location::LocationPorts<'a>) -> Result<(), String>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self { id: id.into(), register: Arc::new(register) }
    }

    /// The plugin id (`"opencode.*"` for the built-ins).
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    fn register_at(
        &self,
        hooks: &HookRegistry,
        ports: &crate::location::LocationPorts<'_>,
    ) -> Result<(), String> {
        (self.register)(hooks, ports)
    }
}

/// Boots the per-location plugin set in the v2 supervisor order
/// (`supervisor.ts:125-150`): internal **pre** plugins → host plugins
/// (sdk + instance + packages; placeholder slot until the plugin lane) →
/// internal **post** plugins.
///
/// - **Dedup**: the first occurrence in boot order wins; later duplicates
///   are reported as failures (v2: "Registry activation dies on a
///   duplicate ID … keep the first occurrence and report later ones").
/// - **Error containment**: one plugin's registration failure is recorded
///   and the boot continues with the remaining plugins.
/// - **Reserved ids**: internal slots may carry `"opencode."` ids; the
///   host lane rejects them for guest plugins via
///   [`is_reserved_plugin_id`].
pub struct PluginSupervisor {
    pre: Vec<InternalPlugin>,
    post: Vec<InternalPlugin>,
    /// Host-plugin slot (`TODO(plugin-lane)`): sdk plugins, instance-bound
    /// plugins and config package plugins boot between pre and post.
    host_slot: (),
}

impl fmt::Debug for PluginSupervisor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginSupervisor")
            .field("pre", &self.pre)
            .field("post", &self.post)
            .finish_non_exhaustive()
    }
}

impl Default for PluginSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginSupervisor {
    /// Empty supervisor (no internal plugins).
    #[must_use]
    pub fn new() -> Self {
        Self { pre: Vec::new(), post: Vec::new(), host_slot: () }
    }

    /// Supervisor with the internal pre/post slots (v2
    /// `PluginInternal.list() → { pre, post }`).
    #[must_use]
    pub fn with_internal(pre: Vec<InternalPlugin>, post: Vec<InternalPlugin>) -> Self {
        Self { pre, post, host_slot: () }
    }

    /// The internal pre plugins (boot order reference).
    #[must_use]
    pub fn pre(&self) -> &[InternalPlugin] {
        &self.pre
    }

    /// The internal post plugins (boot order reference).
    #[must_use]
    pub fn post(&self) -> &[InternalPlugin] {
        &self.post
    }

    /// Runs one plugin with dedup + containment; returns whether it
    /// registered.
    fn boot_one(
        plugin: &InternalPlugin,
        hooks: &HookRegistry,
        ports: &crate::location::LocationPorts<'_>,
        seen: &mut Vec<String>,
        outcome: &mut PluginBootOutcome,
    ) {
        if seen.contains(&plugin.id) {
            // First occurrence in boot order wins; report the duplicate.
            outcome.failures.push(PluginFailure {
                id: plugin.id.clone(),
                error: format!("Duplicate plugin ID: {}", plugin.id),
            });
            return;
        }
        seen.push(plugin.id.clone());
        match plugin.register_at(hooks, ports) {
            Ok(()) => outcome.registered.push(plugin.id.clone()),
            Err(error) => outcome.failures.push(PluginFailure { id: plugin.id.clone(), error }),
        }
    }

    /// Boot every plugin into a fresh [`HookRegistry`] in supervisor order
    /// (pre → host placeholder → post). Never fails the location: failures
    /// are contained in the outcome (v2 reports them as failed plugin
    /// states and activates the rest).
    #[must_use]
    pub fn boot(
        &self,
        hooks: &HookRegistry,
        ports: &crate::location::LocationPorts<'_>,
    ) -> PluginBootOutcome {
        let mut outcome = PluginBootOutcome::default();
        let mut seen: Vec<String> = Vec::new();
        for plugin in &self.pre {
            Self::boot_one(plugin, hooks, ports, &mut seen, &mut outcome);
        }
        // Host slot: sdk + instance + package plugins register here
        // (`TODO(plugin-lane)`); nothing to boot in Wave 0.
        let () = self.host_slot;
        for plugin in &self.post {
            Self::boot_one(plugin, hooks, ports, &mut seen, &mut outcome);
        }
        outcome
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    // The `assert!(x.is_empty())` spelling reads better than the lint's
    // suggested `assert_eq!(x, [])` form.
    #![allow(clippy::assert_is_empty)]

    use super::*;
    use oc_schema::id::{ModelId, ProviderId};
    use serde_json::json;

    fn prompt_event(text: &str) -> SessionPromptEvent {
        SessionPromptEvent {
            session_id: SessionId::new(),
            message_id: MessageId::new(),
            prompt: Prompt { text: text.into(), files: None, agents: None, skills: None },
            metadata: None,
            delivery: Delivery::Queue,
        }
    }

    fn model_ref(provider: &str, model: &str) -> ModelRef {
        ModelRef {
            id: ModelId::parse(model),
            provider_id: ProviderId::parse(provider),
            variant: None,
        }
    }

    #[tokio::test]
    async fn hooks_session_prompt_ordering_and_mutation_chain() {
        let registry = HookRegistry::new();
        let scope = HookScope::new();
        for prefix in ["one:", "two:", "three:"] {
            let registration = registry
                .register_session_prompt(
                    fn_hook(move |event: &mut SessionPromptEvent| {
                        event.prompt.text = format!("{prefix}{}", event.prompt.text);
                        Ok(())
                    }),
                    HookOptions::default(),
                )
                .await;
            scope.track(registration);
        }
        let result = registry.dispatch_session_prompt(prompt_event("go")).await.unwrap();
        // each handler prepends in registration order: one → two → three wrap
        // (reverse order would produce "one:two:three:go")
        assert_eq!(result.prompt.text, "three:two:one:go");
        assert_eq!(registry.total_hooks(), 3);
    }

    #[tokio::test]
    async fn hooks_provider_scoping_skips_non_matching() {
        let registry = HookRegistry::new();
        let scope = HookScope::new();
        let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let registration = registry
            .register_session_model_request(
                {
                    let seen = Arc::clone(&seen);
                    fn_hook(move |event: &mut SessionModelRequestEvent| {
                        seen.lock().push(event.base_url.clone());
                        Ok(())
                    })
                },
                HookOptions { provider_id: Some("anthropic".into()) },
            )
            .await;
        scope.track(registration);

        let make = |provider: &str| SessionModelRequestEvent {
            session_id: SessionId::new(),
            agent: AgentId::parse("build"),
            model: model_ref(provider, "m"),
            kind: SessionRequestKind::Primary,
            base_url: Some(format!("https://{provider}.test")),
            headers: BTreeMap::new(),
        };
        registry.dispatch_session_model_request(make("openai")).await.unwrap();
        registry.dispatch_session_model_request(make("anthropic")).await.unwrap();
        assert_eq!(*seen.lock(), vec![Some("https://anthropic.test".to_string())]);
        // `has` parity (v2 has(name, providerID?))
        assert!(registry.has_session_model_request(Some("anthropic")));
        assert!(!registry.has_session_model_request(Some("openai")));
        assert!(!registry.has_session_model_request(None));
    }

    #[tokio::test]
    async fn hooks_tool_before_rejection_short_circuits() {
        let registry = HookRegistry::new();
        let scope = HookScope::new();
        let registration = registry
            .register_tool_execute_before(
                fn_hook(|event: &mut ToolExecuteBeforeEvent| {
                    if event.input.get("danger").and_then(Value::as_bool) == Some(true) {
                        return Err(HookError::ToolRejected(ToolError {
                            message: "rejected".into(),
                            error: None,
                            metadata: None,
                        }));
                    }
                    event.input["tag"] = json!("mutated");
                    Ok(())
                }),
                HookOptions::default(),
            )
            .await;
        scope.track(registration);

        let ok = ToolExecuteBeforeEvent {
            tool: "bash".into(),
            session_id: SessionId::new(),
            agent: AgentId::parse("build"),
            message_id: MessageId::new(),
            call_id: "cal_1".into(),
            input: json!({"command": "ls"}),
        };
        let ok = registry.dispatch_tool_execute_before(ok).await.unwrap();
        assert_eq!(ok.input["tag"], json!("mutated"));

        let bad = ToolExecuteBeforeEvent { input: json!({"danger": true}), ..ok };
        let err = registry.dispatch_tool_execute_before(bad).await.unwrap_err();
        assert!(matches!(err, HookError::ToolRejected(_)));
    }

    #[tokio::test]
    async fn hooks_scope_disposal_removes_registrations() {
        let registry = HookRegistry::new();
        {
            let scope = HookScope::new();
            let registration = registry
                .register_shell_create_before(
                    fn_hook(|event: &mut ShellCreateBeforeEvent| {
                        event.command = format!("echo {}", event.command);
                        Ok(())
                    }),
                    HookOptions::default(),
                )
                .await;
            scope.track(registration);
            assert!(registry.has_shell_create_before(None));
        } // scope dropped
        assert!(!registry.has_shell_create_before(None));
        assert_eq!(registry.total_hooks(), 0);
        let event = ShellCreateBeforeEvent {
            command: "ls".into(),
            cwd: "/".into(),
            timeout_ms: 1000,
            shell: "bash".into(),
            env: BTreeMap::new(),
        };
        let event = registry.dispatch_shell_create_before(event).await.unwrap();
        assert_eq!(event.command, "ls"); // untouched after disposal
    }

    #[tokio::test]
    async fn hooks_transform_edit_register_reload() {
        let domains = TransformDomains::new();
        let domain = domains.domain::<BTreeMap<String, String>>(DomainId::Skill);

        // base install + registered replayable editor
        domain.reload_with_base(BTreeMap::from([("base".into(), "1".into())]));
        let registration = domain
            .register(Box::new(|state: &mut BTreeMap<String, String>| {
                state.insert("plugin".into(), "on".into());
            }))
            .await;
        assert_eq!(domain.state().get("plugin").map(String::as_str), Some("on"));

        // unregistered edit: visible until reload
        domain.edit(|state| {
            state.insert("ephemeral".into(), "yes".into());
        });
        assert_eq!(domain.state().get("ephemeral").map(String::as_str), Some("yes"));
        let version_before = domain.version();
        domain.reload();
        assert!(domain.version() > version_before);
        assert_eq!(domain.state().get("ephemeral"), None); // dropped by re-derive
        assert_eq!(domain.state().get("plugin").map(String::as_str), Some("on")); // replayed
        assert_eq!(domain.state().get("base").map(String::as_str), Some("1"));

        // disposal removes the replayable editor
        registration.dispose().await;
        assert_eq!(domain.state().get("plugin"), None);
        assert_eq!(domain.state().get("base").map(String::as_str), Some("1"));
    }

    #[tokio::test]
    async fn hooks_transform_registration_order_replays_in_order() {
        let domains = TransformDomains::new();
        let domain = domains.domain::<Vec<String>>(DomainId::Command);
        let scope = HookScope::new();
        for word in ["first", "second", "third"] {
            let registration = domain
                .register(Box::new(move |state: &mut Vec<String>| {
                    state.push(word.to_string());
                }))
                .await;
            scope.track(registration);
        }
        assert_eq!(domain.state(), vec!["first", "second", "third"]);
        domain.edit(|state| state.push("unregistered".into()));
        domain.reload();
        assert_eq!(domain.state(), vec!["first", "second", "third"]);
        scope.close().await;
        assert!(domain.state().is_empty());
    }

    #[tokio::test]
    async fn hooks_event_subscribe_broadcast() {
        let registry = HookRegistry::new();
        let mut receiver = registry.subscribe();
        let delivered = registry.publish_event(EventEnvelope {
            id: "evt_1".into(),
            created: 1,
            r#type: "session.created".into(),
            location: None,
            data: json!({"hello": true}),
        });
        assert_eq!(delivered, 1);
        let event = tokio::time::timeout(std::time::Duration::from_secs(1), receiver.recv())
            .await
            .expect("event arrives")
            .expect("channel open");
        assert_eq!(event.r#type, "session.created");
        assert_eq!(event.data, json!({"hello": true}));
    }

    #[tokio::test]
    async fn hooks_hook_point_keys_cover_v2_surface() {
        let expected = [
            "session.prompt",
            "session.context",
            "session.compaction",
            "session.generate",
            "session.title",
            "session.model.request",
            "session.http.request",
            "session.http.response",
            "session.experimental.ws.handshake",
            "session.experimental.ws.send",
            "session.experimental.ws.receive",
            "session.retry",
            "tool.execute.before",
            "tool.execute.after",
            "permission.evaluate",
            "shell.create.before",
            "aisdk.sdk",
            "aisdk.language",
        ];
        let keys: Vec<&'static str> = HookPoint::all().iter().map(HookPoint::key).collect();
        assert_eq!(keys, expected);
    }

    #[tokio::test]
    async fn hooks_permission_mutation() {
        let registry = HookRegistry::new();
        let scope = HookScope::new();
        let registration = registry
            .register_permission_evaluate(
                fn_hook(|event: &mut PermissionEvaluateEvent| {
                    event.effect = PermissionEffect::Deny;
                    event.message = Some("blocked by policy".into());
                    Ok(())
                }),
                HookOptions::default(),
            )
            .await;
        scope.track(registration);
        let event = PermissionEvaluateEvent {
            session_id: SessionId::new(),
            agent: None,
            action: "bash".into(),
            resources: vec!["*".into()],
            metadata: None,
            source: None,
            effect: PermissionEffect::Ask,
            message: None,
        };
        let event = registry.dispatch_permission_evaluate(event).await.unwrap();
        assert_eq!(event.effect, PermissionEffect::Deny);
        assert_eq!(event.message.as_deref(), Some("blocked by policy"));
    }

    #[tokio::test]
    async fn hooks_aisdk_registration_and_scope_close() {
        let registry = HookRegistry::new();
        let scope = HookScope::new();
        let registration = registry
            .register_aisdk_language(
                fn_hook(|_event: &mut AisdkLanguageEvent| Ok(())),
                HookOptions::default(),
            )
            .await;
        scope.track(registration);
        assert!(registry.has_aisdk_language(None));
        assert_eq!(scope.len(), 1);
        scope.close().await;
        assert!(scope.is_empty());
        assert!(!registry.has_aisdk_language(None));
    }

    // ------------------------------------------------- plugin supervisor

    use crate::bus::{Bus, BusOptions};
    use crate::database::Database;
    use crate::location::{
        AgentRegistry, FileAccessState, InstructionSlots, LocationPorts, LocationRef, ShellEnv,
        ToolRegistryState,
    };
    use crate::provider_catalog::Catalog;
    use oc_util::determinism::FixedClock;
    use std::path::PathBuf;

    /// Real per-location state bundle backing a [`LocationPorts`] (the
    /// graph lane tests cover the full graph; the supervisor tests only
    /// need the ports surface).
    struct PortsBundle {
        db: Arc<Database>,
        bus: Arc<Bus>,
        catalog: Arc<Catalog>,
        agents: Arc<AgentRegistry>,
        tools: Arc<ToolRegistryState>,
        shell: Arc<ShellEnv>,
        files: Arc<FileAccessState>,
        instructions: Arc<InstructionSlots>,
        location: LocationRef,
        hooks: Arc<HookRegistry>,
    }

    impl PortsBundle {
        async fn new() -> Self {
            let db = Arc::new(Database::open_in_memory().await.expect("database"));
            let bus = Arc::new(Bus::new(
                (*db).clone(),
                BusOptions { clock: Arc::new(FixedClock::epoch()), ..BusOptions::deterministic() },
            ));
            Self {
                db,
                bus,
                catalog: Arc::new(Catalog::empty()),
                agents: Arc::new(AgentRegistry::new()),
                tools: Arc::new(ToolRegistryState::new()),
                shell: Arc::new(ShellEnv::new(PathBuf::from("/repo"))),
                files: Arc::new(FileAccessState::new(
                    PathBuf::from("/repo"),
                    PathBuf::from("/repo"),
                    PathBuf::from("/home/u"),
                )),
                instructions: Arc::new(InstructionSlots::new()),
                location: LocationRef::new(PathBuf::from("/repo"), None),
                hooks: Arc::new(HookRegistry::new()),
            }
        }

        fn ports(&self) -> LocationPorts<'_> {
            LocationPorts {
                location: &self.location,
                db: &self.db,
                bus: &self.bus,
                catalog: &self.catalog,
                agents: &self.agents,
                tools: &self.tools,
                shell: &self.shell,
                files: &self.files,
                instructions: &self.instructions,
            }
        }
    }

    fn recording_plugin(id: &'static str, trace: Arc<Mutex<Vec<String>>>) -> InternalPlugin {
        InternalPlugin::new(id, move |_hooks, _ports| {
            trace.lock().push(id.to_string());
            Ok(())
        })
    }

    #[tokio::test]
    async fn plugins_boot_order_pre_host_post() {
        let trace: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let supervisor = PluginSupervisor::with_internal(
            vec![
                recording_plugin("opencode.pre-one", Arc::clone(&trace)),
                recording_plugin("opencode.pre-two", Arc::clone(&trace)),
            ],
            vec![recording_plugin("opencode.post-one", Arc::clone(&trace))],
        );
        let bundle = PortsBundle::new().await;
        let outcome = supervisor.boot(&bundle.hooks, &bundle.ports());
        assert!(outcome.failures.is_empty());
        // pre (in order) → host (nothing yet) → post.
        assert_eq!(
            *trace.lock(),
            vec!["opencode.pre-one", "opencode.pre-two", "opencode.post-one"]
        );
        assert_eq!(outcome.registered.len(), 3);
    }

    #[tokio::test]
    async fn plugins_registration_failure_is_contained() {
        let trace: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let supervisor = PluginSupervisor::with_internal(
            vec![InternalPlugin::new("opencode.boom", |_hooks, _ports| {
                Err("setup exploded".into())
            })],
            vec![recording_plugin("opencode.post", Arc::clone(&trace))],
        );
        let bundle = PortsBundle::new().await;
        let outcome = supervisor.boot(&bundle.hooks, &bundle.ports());
        assert_eq!(
            outcome.failures,
            vec![PluginFailure { id: "opencode.boom".into(), error: "setup exploded".into() }],
            "one failure, contained"
        );
        // The boot continued with the remaining plugins.
        assert_eq!(*trace.lock(), vec!["opencode.post"]);
        assert_eq!(outcome.registered.len(), 1);
    }

    #[tokio::test]
    async fn plugins_duplicate_ids_keep_first_and_report_later() {
        let trace: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let supervisor = PluginSupervisor::with_internal(
            vec![
                recording_plugin("opencode.dup", Arc::clone(&trace)),
                recording_plugin("opencode.dup", Arc::clone(&trace)),
            ],
            vec![recording_plugin("opencode.dup", Arc::clone(&trace))],
        );
        let bundle = PortsBundle::new().await;
        let outcome = supervisor.boot(&bundle.hooks, &bundle.ports());
        assert_eq!(*trace.lock(), vec!["opencode.dup"], "first occurrence only");
        assert_eq!(outcome.registered, vec!["opencode.dup".to_string()]);
        assert_eq!(outcome.failures.len(), 2, "pre + post duplicates reported");
        assert!(outcome.failures.iter().all(|failure| failure.id == "opencode.dup"));
    }

    #[test]
    fn plugins_reserved_id_namespace() {
        // v2 guards the built-ins from removal (internal.ts guarded set);
        // the port reserves the whole "opencode." namespace.
        assert!(is_reserved_plugin_id("opencode.provider.opencode"));
        assert!(is_reserved_plugin_id("opencode.config.policy"));
        assert!(is_reserved_plugin_id("opencode."));
        assert!(!is_reserved_plugin_id("my-plugin"));
        assert!(!is_reserved_plugin_id("community.tools"));
    }

    #[tokio::test]
    async fn plugins_register_into_hook_registry_and_state() {
        // A plugin that actually touches its ports: registers a hook AND
        // seeds an agent (the v2 `opencode.agent` plugin shape).
        let supervisor = PluginSupervisor::with_internal(
            vec![InternalPlugin::new("opencode.agent", |hooks, ports| {
                ports.agents.set(oc_schema::agent::Info::default_for(
                    oc_schema::id::AgentId::parse("build"),
                ));
                // Registration closures are sync; the registry's async
                // surface resolves immediately (futures::executor avoids
                // any runtime dependency).
                let registration = futures::executor::block_on(hooks.register_shell_create_before(
                    fn_hook(|event: &mut ShellCreateBeforeEvent| {
                        event.env.insert("SEED".into(), Some("1".into()));
                        Ok(())
                    }),
                    HookOptions::default(),
                ));
                assert!(hooks.has_shell_create_before(None));
                drop(registration); // disposal on drop
                Ok(())
            })],
            vec![],
        );
        let bundle = PortsBundle::new().await;
        let outcome = supervisor.boot(&bundle.hooks, &bundle.ports());
        assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
        assert_eq!(bundle.agents.select(None).id.0, "build", "agent seeded through ports");
    }
}
