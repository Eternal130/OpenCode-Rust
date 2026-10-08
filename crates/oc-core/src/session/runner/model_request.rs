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
}

/// `Prepared.executeTool` outcome — tool failure or a user decline.
pub type ExecuteToolOutcome = Result<ToolRunResult, ToolRunFailure>;

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
    pub execute_tool: Arc<
        dyn Fn(ToolCallData) -> futures::future::BoxFuture<'static, ExecuteToolOutcome>
            + Send
            + Sync,
    >,
}

/// The request preparation service (`SessionModelRequest`).
pub struct SessionModelRequest {
    hooks: Arc<HookRegistry>,
    tools: Arc<ToolRegistryState>,
    app: AppMetadata,
}

fn tool_definition(registration: &crate::location::ToolRegistration) -> ToolDefinition {
    ToolDefinition::new(
        registration.name.clone(),
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
            by_name.insert(tool.name.clone(), tool.clone());
            draft_tools.insert(
                tool.name.clone(),
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
                    let _ = hooks.dispatch_session_retry(event.clone()).await;
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
        let execute_tool = Arc::new(move |call: ToolCallData| {
            let registry = Arc::clone(&registry);
            let hooked_names = hooked_names.clone();
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
                executor.execute(&call.input).await
            }) as futures::future::BoxFuture<'static, ExecuteToolOutcome>
        });

        Ok(Prepared { request, retry, execute_tool })
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
