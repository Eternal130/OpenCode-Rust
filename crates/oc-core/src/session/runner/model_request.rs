//! Port of `session/model-request.ts` (§3.3) — request preparation for each
//! session flow, with the `session.context` hook (tool-identity rename
//! preservation), output limits, routing headers, prompt-cache affinity,
//! unsupported-media replacement, the inline-image budget, and the
//! `model.request` hook.

#![allow(clippy::too_many_lines)]

use std::collections::BTreeMap;
use std::sync::Arc;

use oc_ai::schema::messages::{ContentPart, Message, SystemPart, ToolChoice, ToolDefinition};
use oc_ai::schema::options::GenerationOptions;
use oc_schema::id::AgentId;
use oc_schema::model::Capabilities;
use oc_schema::session_message::Info as MessageInfo;
use serde_json::Value;

use crate::database::schema::SessionRow;
use crate::hooks::HookRegistry;
use crate::location::{ToolRegistryState, ToolRunFailure, ToolRunResult};
use crate::session::runner::context::Loaded;
use crate::session::runner::{to_llm_message, ModelError, ResolvedModel};

/// Inline-image budget triggers (model-request.ts:43-46).
const IMAGE_BYTES_TRIGGER: f64 = 25.0 * 1024.0 * 1024.0;
const IMAGE_BYTES_TARGET: f64 = 15.0 * 1024.0 * 1024.0;
const IMAGE_REMOVED: &str = "[This image was removed to reduce the request size and is no longer visible. Do not make claims about its contents from memory. If needed, retrieve it again with an available tool or ask the user to attach it again.]";

/// Used when the catalog has no output limit for the model.
const OUTPUT_TOKEN_FALLBACK: f64 = 32_000.0;
/// No reply needs more, however much the model allows.
const OUTPUT_TOKEN_MAX: f64 = 256_000.0;
/// A summary never needs more.
const SUMMARY_OUTPUT_MAX: f64 = 32_000.0;
/// Prompt text is estimated at about 4 characters per token.
const ESTIMATE_ERROR: f64 = 0.15;
/// Never ask for less.
const OUTPUT_TOKEN_MIN: f64 = 1_024.0;

/// `App.Metadata` subset the headers need.
#[derive(Debug, Clone)]
pub struct AppMetadata {
    pub name: String,
    pub version: String,
    pub channel: String,
}

impl Default for AppMetadata {
    fn default() -> Self {
        Self { name: "opencode".into(), version: "unknown".into(), channel: "unknown".into() }
    }
}

impl AppMetadata {
    /// The process app identity, mirroring the ref's `App` inputs
    /// (`packages/cli/src/version.ts` + `server-process.ts:92-97`):
    ///
    /// - `name`: `OPENCODE_CLIENT ?? OPENCODE_ARTIFACT` — the artifact
    ///   fallback is `"cli"` (version.ts:8);
    /// - `version`: the workspace pin (`CARGO_PKG_VERSION` = 2.0.24, the
    ///   parity pin of tools/RELEASE.md § version stamping);
    /// - `channel`: the ONE workspace channel source
    ///   ([`oc_util::channel::current`] — v2 bakes a build-time
    ///   `OPENCODE_CHANNEL` define; the port resolves it at runtime,
    ///   P10-DB1), so the UA, the database path, and the service
    ///   registration always agree.
    ///
    /// The release build + the parity rig both run `stable`, so the
    /// request-path UA reads `opencode/stable/2.0.24/cli` exactly like
    /// the pinned reference build.
    #[must_use]
    pub fn detected() -> Self {
        Self {
            name: std::env::var("OPENCODE_CLIENT")
                .ok()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "cli".to_owned()),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            channel: oc_util::channel::current(),
        }
    }
}

/// `App.useragent` (app.ts:24-26).
#[must_use]
pub fn useragent(app: &AppMetadata) -> String {
    format!("opencode/{}/{}/{}", app.channel, app.version, app.name)
}

/// `SessionAffinity.get` — children share the parent's key, forks the fork
/// source's.
#[must_use]
pub fn session_affinity(session: &SessionRow) -> String {
    session
        .parent_id
        .clone()
        .or_else(|| session.fork_session_id.clone())
        .unwrap_or_else(|| session.id.clone())
}

/// `Input.inputTokens` — prompt size, measured by the provider or estimated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputTokens {
    pub measured: f64,
    pub estimated: f64,
}

/// One tool call passed to [`Prepared::execute_tool`].
#[derive(Debug, Clone)]
pub struct ToolCallData {
    pub id: String,
    pub name: String,
    pub input: Value,
    /// The assistant message id (the permission `source.messageID`).
    pub message_id: String,
}

/// The outcome of one prepared tool execution (`Prepared.executeTool` —
/// tool failure or a user decline).
pub type ExecuteToolOutcome = Result<ToolRunResult, ToolRunFailure>;

/// The prepared request's tool executor closure ([`Prepared::execute_tool`]).
pub type ExecuteTool = Arc<
    dyn Fn(ToolCallData) -> futures::future::BoxFuture<'static, ExecuteToolOutcome> + Send + Sync,
>;

/// The request-preparation input (model-request.ts `Input`).
pub struct RequestInput {
    pub session: SessionRow,
    pub agent: AgentId,
    pub model: ResolvedModel,
    /// `None` advertises no tools for this request.
    pub tools: Option<Vec<crate::location::ToolRegistration>>,
    pub system: Vec<SystemPart>,
    pub messages: Vec<Message>,
    pub tool_choice: Option<ToolChoice>,
    /// Only the durable runner may use a stateful WebSocket (Wave D seam).
    pub web_socket: bool,
    pub input_tokens: Option<InputTokens>,
}

/// `baseTranscript` (model-request.ts:100-120) — system parts + canonical
/// messages for a loaded context.
pub struct Transcript {
    pub provider_metadata_key: String,
    pub system: Vec<SystemPart>,
    pub messages: Vec<Message>,
}

#[must_use]
pub fn base_transcript(
    agent: &oc_schema::agent::Info,
    model: &ResolvedModel,
    tools: &[crate::location::ToolRegistration],
    initial: &str,
    messages: &[MessageInfo],
) -> Transcript {
    let provider_metadata_key = model.provider_metadata_key().to_string();
    let system_text = agent.system.clone().unwrap_or_else(|| {
        crate::session::runner::system_prompt::make(&crate::session::runner::context::tool_names(
            tools,
        ))
    });
    let system: Vec<SystemPart> = [system_text, initial.to_string()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(SystemPart::new)
        .collect();
    let lowered =
        to_llm_message::to_llm_messages(messages, &model.reference, &provider_metadata_key);
    Transcript { provider_metadata_key, system, messages: lowered }
}

/// `outputLimit` (model-request.ts:87-98): the catalog limit, fitted to the
/// room the prompt leaves in the context window.
#[must_use]
pub fn output_limit(
    limit: &oc_schema::model::ModelLimit,
    kind: RequestKind,
    input_tokens: Option<InputTokens>,
) -> f64 {
    let model = (if limit.output > 0 { limit.output as f64 } else { OUTPUT_TOKEN_FALLBACK })
        .min(OUTPUT_TOKEN_MAX);
    let requested = match kind {
        RequestKind::Compaction => model.min(SUMMARY_OUTPUT_MAX),
        _ => model,
    };
    let Some(input_tokens) = input_tokens else { return requested };
    if limit.context <= 0 {
        return requested;
    }
    let room = limit.context as f64
        - input_tokens.measured
        - (input_tokens.estimated * (1.0 + ESTIMATE_ERROR)).ceil();
    requested.min(OUTPUT_TOKEN_MIN.max(room))
}

/// The four request kinds (`SessionRequestKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    Primary,
    Compaction,
    Title,
    Generate,
}

impl RequestKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RequestKind::Primary => "primary",
            RequestKind::Compaction => "compaction",
            RequestKind::Title => "title",
            RequestKind::Generate => "generate",
        }
    }
}

fn mime_to_modality(mime: &str) -> Option<&'static str> {
    if mime.starts_with("image/") {
        Some("image")
    } else if mime.starts_with("audio/") {
        Some("audio")
    } else if mime.starts_with("video/") {
        Some("video")
    } else if mime == "application/pdf" {
        Some("pdf")
    } else {
        None
    }
}

fn unsupported_media(
    mime: &str,
    name: Option<&str>,
    capabilities: &Capabilities,
) -> Option<String> {
    let modality = mime_to_modality(mime)?;
    if capabilities.input.iter().any(|item| item.starts_with(modality)) {
        return None;
    }
    Some(format!(
        "ERROR: Cannot read {} (this model does not support {} input). Inform the user.",
        name.map_or_else(|| modality.to_string(), |name| format!("\"{name}\"")),
        modality
    ))
}

/// Remote and provider-referenced media carry no local payload and never
/// count toward the inline budget.
fn media_bytes(media: &oc_ai::schema::media::Asset) -> f64 {
    use oc_ai::schema::media::Source;
    match &media.source {
        Source::Base64 { data, .. } => data.len() as f64,
        Source::Bytes { data, .. } => (data.0.len() as f64 / 3.0).ceil() * 4.0,
        _ => 0.0,
    }
}

fn replace_media(
    messages: Vec<Message>,
    replace: &dyn Fn(&str, Option<&str>, f64) -> Option<String>,
) -> Vec<Message> {
    messages
        .into_iter()
        .map(|message| {
            let content: Vec<ContentPart> = message
                .content
                .into_iter()
                .map(|part| match part {
                    ContentPart::Media { media, filename, cache, metadata, provider_metadata } => {
                        let text =
                            replace(&media.media_type, filename.as_deref(), media_bytes(&media));
                        match text {
                            Some(text) => {
                                ContentPart::Text { text, cache, metadata, provider_metadata }
                            }
                            None => ContentPart::Media {
                                media,
                                filename,
                                cache,
                                metadata,
                                provider_metadata,
                            },
                        }
                    }
                    ContentPart::ToolResult {
                        id,
                        name,
                        namespace,
                        result,
                        provider_executed,
                        cache,
                        metadata,
                        provider_metadata,
                    } => {
                        let oc_ai::schema::messages::ToolResultValue::Content { value } = &result
                        else {
                            return ContentPart::ToolResult {
                                id,
                                name,
                                namespace,
                                result,
                                provider_executed,
                                cache,
                                metadata,
                                provider_metadata,
                            };
                        };
                        let replaced: Vec<Value> = value
                            .0
                            .iter()
                            .map(|item| {
                                let Some(object) = item.as_object() else { return item.clone() };
                                if object.get("type").and_then(Value::as_str) != Some("file") {
                                    return item.clone();
                                }
                                let mime =
                                    object.get("mime").and_then(Value::as_str).unwrap_or_default();
                                let name = object.get("name").and_then(Value::as_str);
                                let uri =
                                    object.get("uri").and_then(Value::as_str).unwrap_or_default();
                                match replace(mime, name, uri.len() as f64) {
                                    Some(text) => {
                                        serde_json::json!({ "type": "text", "text": text })
                                    }
                                    None => item.clone(),
                                }
                            })
                            .collect();
                        ContentPart::ToolResult {
                            id,
                            name,
                            namespace,
                            result: oc_ai::schema::messages::ToolResultValue::Content {
                                value: oc_ai::schema::messages::ToolContentItems(replaced),
                            },
                            provider_executed,
                            cache,
                            metadata,
                            provider_metadata,
                        }
                    }
                    other => other,
                })
                .collect();
            Message { content, ..message }
        })
        .collect()
}

/// `unsupportedParts` — unsupported modality media → replacement text.
#[must_use]
pub fn unsupported_parts(messages: Vec<Message>, capabilities: &Capabilities) -> Vec<Message> {
    replace_media(messages, &|mime, name, _| unsupported_media(mime, name, capabilities))
}

/// `boundImages` (model-request.ts:172-199): over 25 MiB of inline images,
/// drop oldest until ≤15 MiB, replacing each with [`IMAGE_REMOVED`].
#[must_use]
pub fn bound_images(messages: Vec<Message>) -> Vec<Message> {
    let is_image = |mime: &str| mime.to_ascii_lowercase().starts_with("image/");
    let image_bytes: f64 = messages.iter().fold(0.0, |total, message| {
        total
            + message.content.iter().fold(0.0, |sum, part| match part {
                ContentPart::Media { media, .. } if is_image(&media.media_type) => {
                    sum + media_bytes(media)
                }
                ContentPart::ToolResult {
                    result: oc_ai::schema::messages::ToolResultValue::Content { value },
                    ..
                } => {
                    sum + value.0.iter().fold(0.0, |bytes, item| {
                        bytes
                            + if item.get("type").and_then(Value::as_str) == Some("file")
                                && item
                                    .get("mime")
                                    .and_then(Value::as_str)
                                    .is_some_and(|mime| is_image(mime))
                            {
                                item.get("uri")
                                    .and_then(Value::as_str)
                                    .map_or(0.0, |uri| uri.len() as f64)
                            } else {
                                0.0
                            }
                    })
                }
                _ => sum,
            })
    });
    if image_bytes <= IMAGE_BYTES_TRIGGER {
        return messages;
    }
    let removed = std::cell::Cell::new(0.0f64);
    replace_media(messages, &move |mime, _name, bytes| {
        if !is_image(mime) || image_bytes - removed.get() <= IMAGE_BYTES_TARGET {
            return None;
        }
        removed.set(removed.get() + bytes);
        Some(IMAGE_REMOVED.to_string())
    })
}

/// The generation-keys split of a hook-shaped options record.
fn split_options(
    record: &serde_json::Map<String, Value>,
) -> (GenerationOptions, BTreeMap<String, Value>) {
    let generation: GenerationOptions =
        serde_json::from_value(Value::Object(record.clone())).unwrap_or_default();
    let mut provider_options = BTreeMap::new();
    for (key, value) in record {
        if !GENERATION_KEYS.contains(&key.as_str()) {
            provider_options.insert(key.clone(), value.clone());
        }
    }
    (generation, provider_options)
}

const GENERATION_KEYS: [&str; 8] = [
    "maxTokens",
    "temperature",
    "topP",
    "topK",
    "frequencyPenalty",
    "presencePenalty",
    "seed",
    "stop",
];

/// One prepared request (model-request.ts `Prepared`).
pub struct Prepared {
    pub request: oc_ai::schema::messages::LLMRequest,
    /// The `session.retry` hook caller (retry.rs) — mutates the event so a
    /// plugin decision override propagates.
    pub retry: crate::session::runner::retry::PreparedRetryHook,
    /// Runs a tool call against the tools this request advertised
    /// (`Prepared.executeTool`), recovering declines to typed failures.
    pub execute_tool: ExecuteTool,
    /// PLUG-M02: the `experimental.ws.*` seams for a durable-session
    /// WebSocket transport (`Prepared`'s `options.webSocket` — the
    /// channel executors ride them). `None` when no hooks registered.
    pub web_socket_seams: Option<oc_ai::schema::options::WebSocketSeams>,
}

/// The request preparation service (`SessionModelRequest`).
pub struct SessionModelRequest {
    hooks: Arc<HookRegistry>,
    tools: Arc<ToolRegistryState>,
    app: AppMetadata,
}

fn tool_definition(registration: &crate::location::ToolRegistration) -> ToolDefinition {
    // `definition(tool).name = effectiveName(tool)` (runtime.ts:22 — the
    // model sees `opencode_session_move`, not the bare registration key).
    ToolDefinition::new(
        registration.effective_name(),
        registration.description.clone(),
        registration.input.clone(),
    )
}

impl SessionModelRequest {
    pub fn new(hooks: Arc<HookRegistry>, tools: Arc<ToolRegistryState>, app: AppMetadata) -> Self {
        Self { hooks, tools, app }
    }

    /// `prepare` (model-request.ts:219-401).
    pub async fn prepare(
        &self,
        kind: RequestKind,
        input: RequestInput,
    ) -> Result<Prepared, ModelError> {
        let session = &input.session;
        let model = &input.model;
        let tools = input.tools.unwrap_or_default();
        // Remember which tool each definition came from: hooks rename a tool
        // by moving its definition to a new key, so after the hook the tool
        // is found by identity (description + schema) first, then by key.
        let mut given: BTreeMap<(String, String), crate::location::ToolRegistration> =
            BTreeMap::new();
        let mut by_name: BTreeMap<String, crate::location::ToolRegistration> = BTreeMap::new();
        let mut draft_tools: BTreeMap<String, crate::hooks::ToolDescriptor> = BTreeMap::new();
        for tool in &tools {
            let definition = tool_definition(tool);
            given.insert(
                (definition.description.clone(), canonical(&definition.input_schema)),
                tool.clone(),
            );
            // The draft/context-hook keys are the EFFECTIVE names — v2's
            // tool table is keyed by `effectiveName` (tool.ts:188).
            let advertised = tool.effective_name();
            by_name.insert(advertised.clone(), tool.clone());
            draft_tools.insert(
                advertised,
                crate::hooks::ToolDescriptor {
                    description: definition.description,
                    input: definition.input_schema,
                },
            );
        }

        // Hooks see the default output limit and may change or remove it;
        // titles and generate keep the provider default.
        let mut options = serde_json::Map::new();
        if matches!(kind, RequestKind::Primary | RequestKind::Compaction) {
            let limit = output_limit(&model.limit, kind, input.input_tokens);
            options.insert("maxTokens".into(), serde_json::json!(limit));
        }

        // The `session.{context,compaction,title,generate}` hook.
        let (shaped_system, shaped_messages, shaped_options, shaped_tools) = match kind {
            RequestKind::Title => {
                let event = crate::hooks::SessionTitleEvent {
                    request: crate::hooks::SessionRequestEvent {
                        session_id: session_id_of(session),
                        model: model.reference.clone(),
                        system: input.system.clone(),
                        messages: input.messages.clone(),
                        options: options.clone().into_iter().collect(),
                    },
                    result: None,
                };
                let shaped = self
                    .hooks
                    .dispatch_session_title(event)
                    .await
                    .map_err(|err| ModelError::Resolver(err.to_string()))?;
                let request = shaped.request;
                (request.system, request.messages, record_to_map(&request.options), BTreeMap::new())
            }
            other => {
                let event = crate::hooks::SessionContextEvent {
                    session_id: session_id_of(session),
                    agent: input.agent.clone(),
                    model: model.reference.clone(),
                    system: input.system.clone(),
                    messages: input.messages.clone(),
                    options: options.clone().into_iter().collect(),
                    tools: draft_tools.clone(),
                };
                let shaped = match other {
                    RequestKind::Compaction => self
                        .hooks
                        .dispatch_session_compaction(crate::hooks::SessionCompactionEvent {
                            context: event,
                            result: None,
                        })
                        .await
                        .map(|event| event.context)
                        .map_err(|err| ModelError::Resolver(err.to_string()))?,
                    _ => self
                        .hooks
                        .dispatch_session_context(event)
                        .await
                        .map_err(|err| ModelError::Resolver(err.to_string()))?,
                };
                (shaped.system, shaped.messages, record_to_map(&shaped.options), shaped.tools)
            }
        };

        // Match by identity first, then by key; entries matching neither were
        // invented by a hook and are dropped.
        let hooked: Vec<(String, crate::location::ToolRegistration)> = shaped_tools
            .iter()
            .filter_map(|(name, descriptor)| {
                let matched = given
                    .get(&(descriptor.description.clone(), canonical(&descriptor.input)))
                    .cloned()
                    .or_else(|| by_name.get(name).cloned())?;
                Some((
                    name.clone(),
                    crate::location::ToolRegistration {
                        description: descriptor.description.clone(),
                        input: descriptor.input.clone(),
                        ..matched
                    },
                ))
            })
            .collect();

        let (generation, provider_options) = split_options(&shaped_options);
        let affinity = session_affinity(session);
        let mut headers: BTreeMap<String, String> = BTreeMap::new();
        headers.insert("x-opencode-session-id".into(), session.id.clone());
        if let Some(parent) = &session.parent_id {
            headers.insert("x-opencode-parent-session-id".into(), parent.clone());
            headers.insert("x-parent-session-id".into(), parent.clone());
        }
        headers.insert("x-session-affinity".into(), affinity.clone());
        headers.insert("X-Session-Id".into(), affinity.clone());
        headers.insert("User-Agent".into(), useragent(&self.app));
        headers.insert("x-opencode-project".into(), session.project_id.clone());
        headers.insert("x-opencode-session".into(), affinity.clone());
        headers.insert("x-opencode-client".into(), self.app.name.clone());

        let mut request = oc_ai::schema::messages::LLMRequest::new(model.model.clone());
        request.system = shaped_system;
        request.messages = bound_images(unsupported_parts(shaped_messages, &model.capabilities));
        request.tools = hooked
            .iter()
            .map(|(name, registration)| {
                oc_ai::schema::messages::ToolEntry::Definition(ToolDefinition {
                    name: name.clone(),
                    ..tool_definition(registration)
                })
            })
            .collect();
        request.tool_choice = input.tool_choice.clone();
        if !generation.is_empty() {
            request.generation = Some(generation);
        }
        if !provider_options.is_empty() {
            request.provider_options = Some(provider_options.into_iter().collect());
        }
        request.http = Some(oc_ai::schema::options::HttpOptions {
            headers: Some(headers),
            ..Default::default()
        });
        request.prompt_cache_key = Some(
            if affinity.starts_with("ses_")
                && affinity.len() == 68
                && affinity[4..].chars().all(|c| c.is_ascii_hexdigit())
            {
                affinity[4..].to_string()
            } else {
                affinity
            },
        );

        // `session.model.request` — baseURL/headers override.
        let model_hook = self
            .hooks
            .dispatch_session_model_request(crate::hooks::SessionModelRequestEvent {
                session_id: session_id_of(session),
                agent: input.agent.clone(),
                model: model.reference.clone(),
                kind: match kind {
                    RequestKind::Primary => crate::hooks::SessionRequestKind::Primary,
                    RequestKind::Compaction => crate::hooks::SessionRequestKind::Compaction,
                    RequestKind::Title => crate::hooks::SessionRequestKind::Title,
                    RequestKind::Generate => crate::hooks::SessionRequestKind::Generate,
                },
                base_url: model.model.route.endpoint.base_url.clone(),
                headers: request
                    .http
                    .as_ref()
                    .and_then(|http| http.headers.clone())
                    .unwrap_or_default(),
            })
            .await
            .map_err(|err| ModelError::Resolver(err.to_string()))?;
        if let Some(base_url) = &model_hook.base_url {
            if Some(base_url) != request.model.route.endpoint.base_url.as_ref() {
                let route = request.model.route.with(oc_ai::route::RoutePatch {
                    endpoint: Some(oc_ai::route::EndpointPatch {
                        base_url: Some(base_url.clone()),
                        ..Default::default()
                    }),
                    ..Default::default()
                });
                request.model = oc_ai::schema::options::LanguageModel {
                    route: Arc::new(route),
                    ..request.model.clone()
                };
            }
        }
        if let Some(http) = request.http.as_mut() {
            if model_hook.headers.is_empty() {
                http.headers = None;
            } else {
                http.headers = Some(model_hook.headers);
            }
        }

        // PLUG-M02 — the transport-layer hook dispatch (model-request.ts
        // 320-375). `session.http.request`/`session.http.response` wrap
        // EVERY HTTP request the route executes (the WebSocket fallback
        // included): the registered pair rewrites the outbound request
        // and observes/rewrites the answer at the transport seam. The
        // wrap only installs when a hook is registered — the bare path
        // streams unchanged (v2's `hasHttpHooks` gate).
        let http_hook_kind = match kind {
            RequestKind::Primary => crate::hooks::SessionRequestKind::Primary,
            RequestKind::Compaction => crate::hooks::SessionRequestKind::Compaction,
            RequestKind::Title => crate::hooks::SessionRequestKind::Title,
            RequestKind::Generate => crate::hooks::SessionRequestKind::Generate,
        };
        let has_http_hooks =
            self.hooks.has_session_http_request(None) || self.hooks.has_session_http_response(None);
        if has_http_hooks {
            let intercepts = http_interceptors(
                Arc::clone(&self.hooks),
                session_id_of(session),
                input.agent.clone(),
                model.reference.clone(),
                http_hook_kind,
            );
            let wrapped = oc_ai::route::transport::intercepted(
                Arc::clone(&request.model.route.transport),
                intercepts,
            );
            let route = request.model.route.with(oc_ai::route::RoutePatch {
                transport: Some(wrapped),
                ..oc_ai::route::RoutePatch::default()
            });
            request.model = oc_ai::schema::options::LanguageModel {
                route: Arc::new(route),
                ..request.model.clone()
            };
        }

        // PLUG-M02 — the WebSocket seams (model-request.ts:351-375): the
        // durable session's stateful socket offers the three
        // `experimental.ws.*` rewrite points when hooks registered them;
        // the channel executors ride the seams (the `webSocket === true`
        // gate mirrors v2's `input.webSocket === "session"`).
        let web_socket_seams = if input.web_socket
            && (self.hooks.has_session_ws_handshake(None)
                || self.hooks.has_session_ws_send(None)
                || self.hooks.has_session_ws_receive(None))
        {
            Some(ws_seams(
                Arc::clone(&self.hooks),
                session_id_of(session),
                input.agent.clone(),
                model.reference.clone(),
                http_hook_kind,
            ))
        } else {
            None
        };

        // The retry hook caller.
        let retry_hooks = Arc::clone(&self.hooks);
        let retry_session_id = session_id_of(session);
        let retry_agent = input.agent.clone();
        let retry_model = model.reference.clone();
        let retry: crate::session::runner::retry::PreparedRetryHook =
            Arc::new(move |event: &mut crate::hooks::SessionRetryEvent| {
                let hooks = Arc::clone(&retry_hooks);
                let session_id = retry_session_id.clone();
                let agent = retry_agent.clone();
                let model = retry_model.clone();
                Box::pin(async move {
                    event.session_id = session_id;
                    event.agent = agent;
                    event.model = model;
                    // v2's retry caller (retry.ts:80-87) discards the
                    // trigger's return and reads its LOCAL event after
                    // the hook — plugin mutations flow back BY
                    // REFERENCE (the flagship's foreground-fallback
                    // rewrites `event.decision`). Write the dispatched
                    // event back through the `&mut` so the caller — and
                    // any future guest-bridge adapter — observes the
                    // same mutations.
                    if let Ok(shaped) = hooks.dispatch_session_retry(event.clone()).await {
                        *event = shaped;
                    }
                })
            });

        // The tool executor for this request: resolve from the registry by
        // the ORIGINAL registration name (the hooked name maps back through
        // the snapshot), recovering declines to typed failures.
        let registry = Arc::clone(&self.tools);
        let hooked_names: BTreeMap<String, String> = hooked
            .iter()
            .map(|(name, registration)| (name.clone(), registration.name.clone()))
            .collect();
        // Context captured per request (v2 Tool.Context: session + agent;
        // the assistant message id joins per call in the step loop).
        let context_session = session_id_of(session);
        let context_agent = input.agent.clone();
        let execute_tool = Arc::new(move |call: ToolCallData| {
            let registry = Arc::clone(&registry);
            let hooked_names = hooked_names.clone();
            let context_session = context_session.clone();
            let context_agent = context_agent.clone();
            Box::pin(async move {
                let original = hooked_names.get(&call.name).cloned().unwrap_or(call.name.clone());
                let Some(executor) = registry.executor(&original) else {
                    return Err(ToolRunFailure::Error {
                        message: format!(
                            "No tool named \"{}\" is currently available. Please use a tool from the available tool list.",
                            call.name
                        ),
                        metadata: None,
                    });
                };
                let context = crate::location::ToolCallContext {
                    session_id: context_session,
                    agent: context_agent,
                    message_id: call.message_id.clone(),
                    call_id: call.id.clone(),
                };
                executor.execute_with_context(&call.input, &context).await
            }) as futures::future::BoxFuture<'static, ExecuteToolOutcome>
        });

        Ok(Prepared { request, retry, execute_tool, web_socket_seams })
    }

    /// `primary` — the context hook path with the loaded tools.
    pub async fn primary(&self, input: RequestInput) -> Result<Prepared, ModelError> {
        self.prepare(RequestKind::Primary, input).await
    }

    /// `compaction`.
    pub async fn compaction(&self, input: RequestInput) -> Result<Prepared, ModelError> {
        self.prepare(RequestKind::Compaction, input).await
    }

    /// `title`.
    pub async fn title(&self, input: RequestInput) -> Result<Prepared, ModelError> {
        self.prepare(RequestKind::Title, input).await
    }
}

fn canonical(value: &Value) -> String {
    crate::session::runner::context::instruction_hash(value)
}

// ---------------------------------------------------------------------------
// PLUG-M02: the transport-layer seam builders (model-request.ts:320-375)
// ---------------------------------------------------------------------------

/// The per-request scope every transport hook event carries (v2 spreads
/// `scope` — session, agent, model, kind — into each trigger).
#[derive(Clone)]
struct TransportHookScope {
    session_id: oc_schema::id::SessionId,
    agent: AgentId,
    model: oc_schema::model::Ref,
    kind: crate::hooks::SessionRequestKind,
}

/// Builds the [`oc_ai::route::transport::HttpInterceptors`] pair
/// dispatching `session.http.request` / `session.http.response` through
/// `hooks`: mutations flow back by value (the dispatched event replaces
/// the outbound request / the buffered response).
fn http_interceptors(
    hooks: Arc<HookRegistry>,
    session_id: oc_schema::id::SessionId,
    agent: AgentId,
    model: oc_schema::model::Ref,
    kind: crate::hooks::SessionRequestKind,
) -> oc_ai::route::transport::HttpInterceptors {
    let scope = TransportHookScope { session_id, agent, model, kind };
    let request_hooks = Arc::clone(&hooks);
    let request_scope = scope.clone();
    let on_request = Arc::new(move |request: oc_ai::route::transport::TransportRequest| {
        let hooks = Arc::clone(&request_hooks);
        let scope = request_scope.clone();
        Box::pin(async move {
            let event = crate::hooks::SessionHttpRequestEvent {
                session_id: scope.session_id.clone(),
                agent: scope.agent.clone(),
                model: scope.model.clone(),
                kind: scope.kind,
                request: crate::hooks::HttpRequest {
                    url: request.url.clone(),
                    method: request.method.clone(),
                    headers: request.headers.iter().cloned().collect(),
                    body: body_to_value(&request.body),
                },
            };
            let shaped = hooks
                .dispatch_session_http_request(event)
                .await
                .map_err(|error| error.to_string())?;
            Ok(oc_ai::route::transport::TransportRequest {
                url: shaped.request.url,
                method: shaped.request.method,
                headers: shaped.request.headers.into_iter().collect(),
                body: value_to_body(&shaped.request.body).into(),
            })
        })
            as futures::future::BoxFuture<
                'static,
                Result<oc_ai::route::transport::TransportRequest, String>,
            >
    });
    let response_hooks = Arc::clone(&hooks);
    let response_scope = scope;
    let on_response = Arc::new(move |response: oc_ai::route::transport::HttpResponseSnapshot| {
        let hooks = Arc::clone(&response_hooks);
        let scope = response_scope.clone();
        Box::pin(async move {
            let event = crate::hooks::SessionHttpResponseEvent {
                session_id: scope.session_id.clone(),
                agent: scope.agent.clone(),
                model: scope.model.clone(),
                kind: scope.kind,
                request: crate::hooks::HttpRequest {
                    url: String::new(),
                    method: String::new(),
                    headers: BTreeMap::new(),
                    body: Value::Null,
                },
                response: crate::hooks::HttpResponse {
                    status: response.status,
                    headers: response.headers,
                    body: body_to_value(&response.body),
                },
            };
            let shaped = hooks
                .dispatch_session_http_response(event)
                .await
                .map_err(|error| error.to_string())?;
            Ok(oc_ai::route::transport::HttpResponseSnapshot {
                status: shaped.response.status,
                headers: shaped.response.headers,
                body: value_to_body(&shaped.response.body),
            })
        })
            as futures::future::BoxFuture<
                'static,
                Result<oc_ai::route::transport::HttpResponseSnapshot, String>,
            >
    });
    oc_ai::route::transport::HttpInterceptors { on_request, on_response }
}

/// Builds the [`oc_ai::schema::options::WebSocketSeams`] triple
/// dispatching the `experimental.ws.*` hooks (model-request.ts:351-372):
/// handshake before the socket opens, per-frame rewrites both ways.
fn ws_seams(
    hooks: Arc<HookRegistry>,
    session_id: oc_schema::id::SessionId,
    agent: AgentId,
    model: oc_schema::model::Ref,
    kind: crate::hooks::SessionRequestKind,
) -> oc_ai::schema::options::WebSocketSeams {
    let scope = TransportHookScope { session_id, agent, model, kind };
    let handshake_hooks = Arc::clone(&hooks);
    let handshake_scope = scope.clone();
    let handshake = Arc::new(move |connect: oc_ai::schema::options::WsHandshake| {
        let hooks = Arc::clone(&handshake_hooks);
        let scope = handshake_scope.clone();
        Box::pin(async move {
            let shaped = hooks
                .dispatch_session_ws_handshake(crate::hooks::SessionWsHandshakeEvent {
                    session_id: scope.session_id.clone(),
                    agent: scope.agent.clone(),
                    model: scope.model.clone(),
                    kind: scope.kind,
                    url: connect.url,
                    headers: connect.headers,
                })
                .await
                .map_err(|error| error.to_string())?;
            Ok(oc_ai::schema::options::WsHandshake { url: shaped.url, headers: shaped.headers })
        })
            as futures::future::BoxFuture<
                'static,
                Result<oc_ai::schema::options::WsHandshake, String>,
            >
    });
    let frame = |hooks: Arc<HookRegistry>, scope: TransportHookScope, outbound: bool| {
        Arc::new(move |frame: String| {
            let hooks = Arc::clone(&hooks);
            let scope = scope.clone();
            Box::pin(async move {
                let event_frame = frame.clone();
                let shaped_frame = if outbound {
                    hooks
                        .dispatch_session_ws_send(crate::hooks::SessionWsSendEvent {
                            session_id: scope.session_id.clone(),
                            agent: scope.agent.clone(),
                            model: scope.model.clone(),
                            kind: scope.kind,
                            frame: event_frame,
                        })
                        .await
                        .map_err(|error| error.to_string())?
                        .frame
                } else {
                    hooks
                        .dispatch_session_ws_receive(crate::hooks::SessionWsReceiveEvent {
                            session_id: scope.session_id.clone(),
                            agent: scope.agent.clone(),
                            model: scope.model.clone(),
                            kind: scope.kind,
                            frame: event_frame,
                        })
                        .await
                        .map_err(|error| error.to_string())?
                        .frame
                };
                Ok(shaped_frame)
            }) as futures::future::BoxFuture<'static, Result<String, String>>
        })
    };
    oc_ai::schema::options::WebSocketSeams {
        handshake,
        send: frame(Arc::clone(&hooks), scope.clone(), true),
        receive: frame(hooks, scope, false),
    }
}

/// Wire bytes → the hook event's `body` view: JSON when the bytes parse
/// (the provider request/response bodies are JSON or SSE text), the raw
/// text otherwise.
fn body_to_value(body: &[u8]) -> Value {
    if let Ok(parsed) = serde_json::from_slice::<Value>(body) {
        return parsed;
    }
    Value::String(String::from_utf8_lossy(body).into_owned())
}

/// The hook's possibly-rewritten `body` view back to wire bytes: a JSON
/// string re-encodes verbatim, any other value serializes as JSON.
fn value_to_body(value: &Value) -> Vec<u8> {
    match value {
        Value::String(text) => text.clone().into_bytes(),
        other => other.to_string().into_bytes(),
    }
}

fn record_to_map(record: &serde_json::Map<String, Value>) -> serde_json::Map<String, Value> {
    record.clone()
}

fn session_id_of(session: &SessionRow) -> oc_schema::id::SessionId {
    oc_schema::id::SessionId::parse(session.id.clone())
        .unwrap_or_else(|err| oc_schema::id::SessionId(err.value))
}

/// A convenience builder over a [`Loaded`] context for `primary` requests.
pub async fn prepare_primary(
    service: &SessionModelRequest,
    loaded: &Loaded,
    tools_tool_choice: Option<ToolChoice>,
    max_steps: bool,
    input_tokens: Option<InputTokens>,
) -> Result<Prepared, ModelError> {
    let transcript = base_transcript(
        &loaded.agent,
        &loaded.model,
        &loaded.tools,
        &loaded.initial,
        &loaded.messages,
    );
    let mut messages = transcript.messages;
    if max_steps {
        messages.push(Message::assistant(vec![ContentPart::text(
            crate::session::runner::max_steps::MAX_STEPS_PROMPT,
        )]));
    }
    service
        .primary(RequestInput {
            session: loaded.session.clone(),
            agent: loaded.agent_id.clone(),
            model: loaded.model.clone(),
            tools: Some(loaded.tools.clone()),
            system: transcript.system,
            messages,
            // Keep tool definitions on the final Step to preserve the
            // provider's cached prefix.
            tool_choice: max_steps.then(ToolChoice::none).or(tools_tool_choice),
            web_socket: true,
            input_tokens,
        })
        .await
}

#[cfg(test)]
mod tools_exec_name_tests {
    // TOOL-A1: the model-facing tool name is the EFFECTIVE name
    // (`<namespace>_<name>` — runtime.ts:22 `definition().name =
    // effectiveName(tool)`; v2 keys the tool table by effectiveName).
    use super::tool_definition;
    use crate::location::ToolRegistration;

    #[test]
    fn model_tool_definitions_use_effective_names() {
        let mut namespaced = ToolRegistration::new(
            "session_move",
            "Move".to_string(),
            serde_json::json!({"type": "object"}),
        );
        namespaced.namespace = Some("opencode".to_string());
        namespaced.codemode = true;
        assert_eq!(tool_definition(&namespaced).name, "opencode_session_move");

        // MCP shape (tool/mcp.ts:44-45): bare tool name + server
        // namespace → effective `server_tool`.
        let mut mcp_style = ToolRegistration::new(
            "create_issue",
            "Issue".to_string(),
            serde_json::json!({"type": "object"}),
        );
        mcp_style.namespace = Some("github".to_string());
        mcp_style.permission = Some("github_create_issue".to_string());
        assert_eq!(tool_definition(&mcp_style).name, "github_create_issue");

        let bare = ToolRegistration::new(
            "read",
            "Read".to_string(),
            serde_json::json!({"type": "object"}),
        );
        assert_eq!(tool_definition(&bare).name, "read");
    }
}
