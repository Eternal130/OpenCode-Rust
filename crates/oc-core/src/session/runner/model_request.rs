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
use crate::session::runner::context::{provenance, provenance_matches, Loaded};
use crate::session::runner::{identity, optimize, plan, to_llm_message, ModelError, ResolvedModel};

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
    // G-193.4 (plugin/optimize.ts): the first matching provider family
    // template rewrites the DEFAULT system prompt (never a custom agent
    // system — optimize.ts:81 skips those).
    let system_text = match agent.system.clone() {
        Some(system) => system,
        None => optimize::apply(
            &crate::session::runner::system_prompt::make(&crate::session::runner::context::tool_names(
                tools,
            )),
            model.info.as_deref(),
            &crate::session::runner::context::tool_names(tools),
        ),
    };
    let mut system: Vec<SystemPart> = [system_text, initial.to_string()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(SystemPart::new)
        .collect();
    // G-193.2 (plugin/identity.ts:27): the "# Your Model" section
    // splices at system index 1, between the base prompt and the
    // initial instructions.
    let identity = identity::identity_for(model.info.as_deref(), &model.reference);
    let at = system.len().min(1);
    system.insert(at, SystemPart::new(identity));
    let lowered =
        to_llm_message::to_llm_messages(messages, &model.reference, &provider_metadata_key);
    let mut lowered = lowered;
    // G-193.1 (plugin/plan.ts:58-74): the per-request Plan-mode reminder
    // reconciliation over the lowered messages. (The ref also persists a
    // synthetic user message; this port injects at request assembly
    // only.)
    {
        let directory = plan::plan_directory(&oc_util::global_roots::resolve().home);
        let enter = plan::enter_reminder(&directory);
        let is_plan = agent.id.as_str() == "plan";
        if let Some(text) = plan::reminder_for(is_plan, plan::last_reminder(&lowered, &enter), &enter)
        {
            plan::inject(&mut lowered, &text);
        }
    }
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
    /// The `session.compaction` hook's short-circuit `result`
    /// (compaction.ts:396): set when a plugin supplied the summary, so
    /// the model call is skipped. Only ever set for `RequestKind::Compaction`.
    pub compaction_result: Option<crate::hooks::CompactionResult>,
    /// The `session.title` hook's short-circuit `result` (title.ts:73):
    /// set when a plugin supplied the title, so the model call is
    /// skipped. Only ever set for `RequestKind::Title`.
    pub title_result: Option<String>,
}

/// The request preparation service (`SessionModelRequest`).
pub struct SessionModelRequest {
    hooks: Arc<HookRegistry>,
    tools: Arc<ToolRegistryState>,
    app: AppMetadata,
    /// RV-G3 (prompt.ts `normalizeImageAttachment`): limits applied to
    /// inline image parts of request messages. `None` = disabled (tests
    /// that pin raw media payloads); the engine always attaches the
    /// config-resolved limits.
    image_limits: Option<crate::image::Limits>,
    /// G-195 test seam: the `web_socket` flag of the LAST prepared
    /// request (native compaction passes the session seam, ref
    /// compaction.ts:335+569-590).
    #[cfg(test)]
    last_web_socket: std::sync::atomic::AtomicBool,
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
        Self {
            hooks,
            tools,
            app,
            image_limits: None,
            #[cfg(test)]
            last_web_socket: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// G-195 test seam: whether the last prepared request carried the
    /// session WebSocket flag.
    #[cfg(test)]
    pub fn test_last_web_socket(&self) -> bool {
        self.last_web_socket.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Attaches the model-input image limits (RV-G3): inline image parts
    /// of request messages are limit-checked / downscaled.
    #[must_use]
    pub fn with_image_limits(mut self, limits: crate::image::Limits) -> Self {
        self.image_limits = Some(limits);
        self
    }

    /// prompt.ts `normalizeImageAttachment` — the prompt-side seam. REF
    /// runs this while the attachment is being read (before storage),
    /// erroring the prompt on decode/size failures; the Rust port
    /// resolves attachments server-side before this seam, so the limits
    /// are enforced at request build time instead, degrading an
    /// over-limit part to the REF omission note rather than failing an
    /// already-stored session.
    fn normalize_messages(&self, messages: Vec<Message>) -> Vec<Message> {
        let Some(limits) = self.image_limits else {
            return messages;
        };
        messages
            .into_iter()
            .map(|mut message| {
                message.content = message
                    .content
                    .into_iter()
                    .map(|part| match part {
                        ContentPart::Media {
                            media,
                            filename,
                            cache,
                            metadata,
                            provider_metadata,
                        } => {
                            if media.kind() != oc_ai::schema::media::Kind::Image {
                                return ContentPart::Media {
                                    media,
                                    filename,
                                    cache,
                                    metadata,
                                    provider_metadata,
                                };
                            }
                            let Some(inline) = media.inline() else {
                                return ContentPart::Media {
                                    media,
                                    filename,
                                    cache,
                                    metadata,
                                    provider_metadata,
                                };
                            };
                            let resource = filename.clone().unwrap_or_else(|| inline.mime.clone());
                            match crate::image::normalize(&resource, &inline.base64, &limits) {
                                Ok(normalized) => {
                                    let mime = if normalized.mime.is_empty() {
                                        inline.mime.clone()
                                    } else {
                                        normalized.mime
                                    };
                                    ContentPart::Media {
                                        media: oc_ai::schema::media::Asset {
                                            source: oc_ai::schema::media::Source::Base64 {
                                                data: normalized.base64,
                                                media_type: mime.clone(),
                                            },
                                            media_type: mime,
                                            ..media
                                        },
                                        filename,
                                        cache,
                                        metadata,
                                        provider_metadata,
                                    }
                                }
                                Err(error) => {
                                    ContentPart::text(format!("[1 image omitted: {error}]"))
                                }
                            }
                        }
                        other => other,
                    })
                    .collect();
                message
            })
            .collect()
    }

    /// `prepare` (model-request.ts:219-401).
    pub async fn prepare(
        &self,
        kind: RequestKind,
        input: RequestInput,
    ) -> Result<Prepared, ModelError> {
        // RV-G3: prompt-side image limits run before the session hooks see
        // the messages (prompt.ts normalizes attachments at read time).
        let input = RequestInput { messages: self.normalize_messages(input.messages), ..input };
        #[cfg(test)]
        self.last_web_socket.store(input.web_socket, std::sync::atomic::Ordering::SeqCst);
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

        // Hook short-circuit results (only the matching kind ever sets one).
        let mut compaction_result = None;
        let mut title_result = None;

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
                // title.ts:73 — a plugin-supplied title short-circuits the
                // model request.
                title_result.clone_from(&shaped.result);
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
                    RequestKind::Compaction => {
                        let shaped = self
                            .hooks
                            .dispatch_session_compaction(crate::hooks::SessionCompactionEvent {
                                context: event,
                                result: None,
                            })
                            .await
                            .map_err(|err| ModelError::Resolver(err.to_string()))?;
                        // compaction.ts:396 — a plugin-supplied summary
                        // short-circuits the model request.
                        compaction_result.clone_from(&shaped.result);
                        shaped.context
                    }
                    // model-request.ts:410 — generate rides the
                    // `session.generate` hook (`agentHook("generate", ...)`),
                    // NOT `session.context`.
                    RequestKind::Generate => self
                        .hooks
                        .dispatch_session_generate(event)
                        .await
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

        // G-65 (ref aisdk.ts:346-354): the sdk/language transform points
        // run at language-model construction — `runSDK` may swap the
        // package instance and `runLanguage` may REPLACE the language
        // model (transform points, not observers). The dispatch feeds
        // the language event the sdk event's handle; a returned
        // language handle becomes the request's target. The options a
        // hook returns ride into the provider options (the copilot
        // lane selection's `endpoint` override flows through here —
        // the inline selection stays the default when no hook runs).
        let mut language_model = model.model.clone();
        let mut provider_options = provider_options;
        let mut sdk_handle: Option<crate::hooks::SdkHandle> = None;
        let mut package = format!("@ai-sdk/{}", model.reference.provider_id.as_str());
        if self.hooks.has_aisdk_sdk(None) {
            let event = crate::hooks::AisdkSdkEvent {
                model: aisdk_model_info(model),
                package: package.clone(),
                options: provider_options.clone().into_iter().collect(),
                sdk: None,
            };
            let shaped = self
                .hooks
                .dispatch_aisdk_sdk(event)
                .await
                .map_err(|err| ModelError::Resolver(err.to_string()))?;
            if !shaped.package.is_empty() {
                package = shaped.package;
            }
            sdk_handle = shaped.sdk;
        }
        if self.hooks.has_aisdk_language(None) {
            let event = crate::hooks::AisdkLanguageEvent {
                model: aisdk_model_info(model),
                sdk: sdk_handle,
                options: provider_options.clone().into_iter().collect(),
                language: Some(model.model.clone()),
            };
            let shaped = self
                .hooks
                .dispatch_aisdk_language(event)
                .await
                .map_err(|err| ModelError::Resolver(err.to_string()))?;
            if let Some(replacement) = shaped.language {
                language_model = replacement;
            }
            for (key, value) in shaped.options {
                provider_options.insert(key, value);
            }
        }
        let mut request = oc_ai::schema::messages::LLMRequest::new(language_model);
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
            // G-206: ref model-request.ts:278 accepts lowercase hex only.
            if affinity.starts_with("ses_")
                && affinity.len() == 68
                && affinity[4..].chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
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

        // ProviderContext/route-hook guard (model-request.ts:304-318):
        // history selects opaque/encrypted compaction windows against the
        // catalog route BEFORE hooks run; a `model.request` hook that
        // reroutes the request must not carry an existing window to
        // another deployment. `prepare` has no error channel, so like hook
        // failures this dies here — the request is never sent.
        if let Some(selected) = provenance(model) {
            let routed = routed_provenance(&request, model);
            if !provenance_matches(&selected, routed.as_ref())
                && request.messages.iter().any(|message| {
                    message
                        .content
                        .iter()
                        .any(|part| matches!(part, ContentPart::Compaction { .. }))
                })
            {
                return Err(ModelError::Resolver(
                    "Provider context is incompatible with the route selected by model request hooks"
                        .to_owned(),
                ));
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

        Ok(Prepared {
            request,
            retry,
            execute_tool,
            web_socket_seams,
            compaction_result,
            title_result,
        })
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

/// The routed provenance of a prepared request (after hooks) — the same
/// helper the compaction path's native guard uses.
pub(crate) fn routed_provenance(
    request: &oc_ai::schema::messages::LLMRequest,
    model: &ResolvedModel,
) -> Option<oc_schema::session_support::Provenance> {
    let routed = ResolvedModel {
        model: request.model.clone(),
        reference: model.reference.clone(),
        info: model.info.clone(),
        capabilities: model.capabilities.clone(),
        cost: model.cost.clone(),
        limit: model.limit,
        compaction: model.compaction,
        transport: model.transport,
    };
    provenance(&routed)
}

fn canonical(value: &Value) -> String {
    crate::session::runner::context::instruction_hash(value)
}

/// The G-65 dispatch's `Model.Info` projection: the resolved model's
/// identity + capabilities over the permissive placeholder shape (the
/// resolver's catalog info is not carried on [`ResolvedModel`]).
fn aisdk_model_info(model: &ResolvedModel) -> oc_schema::model::Info {
    let mut info = oc_schema::model::Info::default_for(
        model.reference.provider_id.clone(),
        model.reference.id.clone(),
    );
    info.capabilities = model.capabilities.clone();
    info
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
    // G-91 — the response event carries the BEFORE-request snapshot
    // (model-request.ts:340 `request: before.request`): the shaped
    // request from the request hook is stashed here so the paired
    // response interceptor can replay it. Intercepted transport runs
    // on_request → handler → on_response sequentially per request, so
    // the stash is always the immediately-preceding request.
    let before_request: Arc<parking_lot::Mutex<Option<crate::hooks::HttpRequest>>> =
        Arc::new(parking_lot::Mutex::new(None));
    let request_hooks = Arc::clone(&hooks);
    let request_scope = scope.clone();
    let request_stash = Arc::clone(&before_request);
    let on_request = Arc::new(move |request: oc_ai::route::transport::TransportRequest| {
        let hooks = Arc::clone(&request_hooks);
        let scope = request_scope.clone();
        let request_stash = Arc::clone(&request_stash);
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
            // G-91 — stash the shaped request for the response event.
            *request_stash.lock() = Some(crate::hooks::HttpRequest {
                url: shaped.request.url.clone(),
                method: shaped.request.method.clone(),
                headers: shaped.request.headers.clone(),
                body: shaped.request.body.clone(),
            });
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
    let response_stash = Arc::clone(&before_request);
    let on_response = Arc::new(move |response: oc_ai::route::transport::HttpResponseSnapshot| {
        let hooks = Arc::clone(&response_hooks);
        let scope = response_scope.clone();
        let response_stash = Arc::clone(&response_stash);
        Box::pin(async move {
            // G-91 — `request: before.request` (model-request.ts:340);
            // an absent stash (a response without a paired request hook)
            // keeps the empty payload.
            let request =
                response_stash.lock().take().unwrap_or_else(|| crate::hooks::HttpRequest {
                    url: String::new(),
                    method: String::new(),
                    headers: BTreeMap::new(),
                    body: Value::Null,
                });
            let event = crate::hooks::SessionHttpResponseEvent {
                session_id: scope.session_id.clone(),
                agent: scope.agent.clone(),
                model: scope.model.clone(),
                kind: scope.kind,
                request,
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
mod hook_dispatch_tests {
    // G-23: `session.generate` fires for generate-kind requests; the
    // context hook does not (model-request.ts:410 — generate rides
    // `agentHook("generate", ...)`, primary rides `agentHook("context")`).
    use super::{AppMetadata, RequestInput, RequestKind, SessionModelRequest};
    use crate::database::schema::SessionRow;
    use crate::hooks::fn_hook;
    use crate::session::runner::ResolvedModel;
    use oc_ai::schema::messages::Message;
    use std::sync::{Arc, Mutex};

    fn resolved() -> ResolvedModel {
        let route =
            oc_ai::test_support::echo_route(Arc::new(oc_ai::route::transport::FailingTransport {
                message: "never sent",
            }));
        let model = route.model("fake-model", Some("parityfake".into())).expect("route model");
        ResolvedModel {
            reference: oc_schema::model::Ref {
                provider_id: oc_schema::id::ProviderId::parse("parityfake"),
                id: oc_schema::id::ModelId::parse("fake-model"),
                variant: Some(oc_schema::id::ModelVariantId::parse("default")),
            },
            model,
            info: None,
            capabilities: oc_schema::model::Capabilities::default(),
            cost: Vec::new(),
            limit: oc_schema::model::ModelLimit { context: 200_000, input: None, output: 32_000 },
            compaction: None,
            transport: None,
        }
    }

    fn input(model: &ResolvedModel, messages: Vec<Message>) -> RequestInput {
        RequestInput {
            session: SessionRow::new("ses_guard_1", "global", "/tmp/opencode", "guard", "test")
                .expect("session row"),
            agent: oc_schema::id::AgentId::parse("build"),
            model: model.clone(),
            tools: None,
            system: Vec::new(),
            messages,
            tool_choice: None,
            web_socket: false,
            input_tokens: None,
        }
    }

    #[tokio::test]
    async fn generate_kind_fires_session_generate_not_context() {
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        let tools = Arc::new(crate::location::ToolRegistryState::new());
        let fired: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let service = SessionModelRequest::new(Arc::clone(&hooks), tools, AppMetadata::default());

        let generate_fired = Arc::clone(&fired);
        std::mem::forget(
            hooks
                .register_session_generate(
                    fn_hook(move |_event: &mut crate::hooks::SessionGenerateEvent| {
                        generate_fired.lock().unwrap().push("generate");
                        Ok(())
                    }),
                    crate::hooks::HookOptions::default(),
                )
                .await,
        );
        let context_fired = Arc::clone(&fired);
        std::mem::forget(
            hooks
                .register_session_context(
                    fn_hook(move |_event: &mut crate::hooks::SessionContextEvent| {
                        context_fired.lock().unwrap().push("context");
                        Ok(())
                    }),
                    crate::hooks::HookOptions::default(),
                )
                .await,
        );

        let model = resolved();
        // Generate kind: only `session.generate` fires.
        service
            .prepare(RequestKind::Generate, input(&model, vec![Message::user("summarize")]))
            .await
            .expect("generate prepare");
        assert_eq!(*fired.lock().unwrap(), vec!["generate"]);

        // Primary kind: only `session.context` fires.
        fired.lock().unwrap().clear();
        service
            .prepare(RequestKind::Primary, input(&model, vec![Message::user("hello")]))
            .await
            .expect("primary prepare");
        assert_eq!(*fired.lock().unwrap(), vec!["context"]);
    }
}

#[cfg(test)]
mod route_guard_tests {
    // G-22: a `model.request` baseURL rewrite incompatible with an opaque
    // compaction window in the prompt dies before the request is sent
    // (model-request.ts:304-318).
    use super::{AppMetadata, RequestInput, RequestKind, SessionModelRequest};
    use crate::database::schema::SessionRow;
    use crate::hooks::fn_hook;
    use crate::session::runner::ResolvedModel;
    use oc_ai::schema::messages::{ContentPart, Message};
    use std::sync::Arc;

    fn resolved() -> ResolvedModel {
        let route =
            oc_ai::test_support::echo_route(Arc::new(oc_ai::route::transport::FailingTransport {
                message: "never sent",
            }));
        let model = route.model("fake-model", Some("parityfake".into())).expect("route model");
        ResolvedModel {
            reference: oc_schema::model::Ref {
                provider_id: oc_schema::id::ProviderId::parse("parityfake"),
                id: oc_schema::id::ModelId::parse("fake-model"),
                variant: Some(oc_schema::id::ModelVariantId::parse("default")),
            },
            model,
            info: None,
            capabilities: oc_schema::model::Capabilities::default(),
            cost: Vec::new(),
            limit: oc_schema::model::ModelLimit { context: 200_000, input: None, output: 32_000 },
            compaction: None,
            transport: None,
        }
    }

    fn input(model: &ResolvedModel, messages: Vec<Message>) -> RequestInput {
        RequestInput {
            session: SessionRow::new("ses_guard_1", "global", "/tmp/opencode", "guard", "test")
                .expect("session row"),
            agent: oc_schema::id::AgentId::parse("build"),
            model: model.clone(),
            tools: None,
            system: Vec::new(),
            messages,
            tool_choice: None,
            web_socket: false,
            input_tokens: None,
        }
    }

    #[tokio::test]
    async fn route_rewrite_with_opaque_window_dies() {
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        let tools = Arc::new(crate::location::ToolRegistryState::new());
        std::mem::forget(
            hooks
                .register_session_model_request(
                    fn_hook(|event: &mut crate::hooks::SessionModelRequestEvent| {
                        event.base_url = Some("https://elsewhere.test".into());
                        Ok(())
                    }),
                    crate::hooks::HookOptions::default(),
                )
                .await,
        );
        let service = SessionModelRequest::new(Arc::clone(&hooks), tools, AppMetadata::default());

        let model = resolved();
        // An opaque compaction window in the prompt + an incompatible
        // baseURL rewrite → die (never send).
        let window = vec![Message::assistant(vec![ContentPart::compaction_encrypted(
            "parityfake",
            "opaque-window",
        )])];
        let error = match service.prepare(RequestKind::Primary, input(&model, window)).await {
            Ok(_) => panic!("incompatible rewrite must die"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains(
                "Provider context is incompatible with the route selected by model request hooks"
            ),
            "unexpected error: {error}"
        );

        // The same rewrite without a compaction window is allowed and the
        // route is patched.
        let prepared = service
            .prepare(RequestKind::Primary, input(&model, vec![Message::user("hello")]))
            .await
            .expect("rewrite without a window is fine");
        assert_eq!(
            prepared.request.model.route.endpoint.base_url.as_deref(),
            Some("https://elsewhere.test")
        );
    }
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

#[cfg(test)]
mod aisdk_dispatch_tests {
    // G-65 (aisdk.ts:346-354): the sdk/language hook pair runs at
    // language-model construction; a language replacement changes the
    // request target, and the sdk handle flows into the language event.
    use super::{AppMetadata, RequestInput, RequestKind, SessionModelRequest};
    use crate::database::schema::SessionRow;
    use crate::hooks::fn_hook;
    use crate::session::runner::ResolvedModel;
    use oc_ai::schema::messages::Message;
    use std::sync::{Arc, Mutex};

    fn resolved() -> ResolvedModel {
        let route =
            oc_ai::test_support::echo_route(Arc::new(oc_ai::route::transport::FailingTransport {
                message: "never sent",
            }));
        let model = route.model("fake-model", Some("parityfake".into())).expect("route model");
        ResolvedModel {
            reference: oc_schema::model::Ref {
                provider_id: oc_schema::id::ProviderId::parse("parityfake"),
                id: oc_schema::id::ModelId::parse("fake-model"),
                variant: Some(oc_schema::id::ModelVariantId::parse("default")),
            },
            model,
            info: None,
            capabilities: oc_schema::model::Capabilities::default(),
            cost: Vec::new(),
            limit: oc_schema::model::ModelLimit { context: 200_000, input: None, output: 32_000 },
            compaction: None,
            transport: None,
        }
    }

    fn input(model: &ResolvedModel, messages: Vec<Message>) -> RequestInput {
        RequestInput {
            session: SessionRow::new("ses_aisdk_1", "global", "/tmp/opencode", "guard", "test")
                .expect("session row"),
            agent: oc_schema::id::AgentId::parse("build"),
            model: model.clone(),
            tools: None,
            system: Vec::new(),
            messages,
            tool_choice: None,
            web_socket: false,
            input_tokens: None,
        }
    }

    fn service(hooks: Arc<crate::hooks::HookRegistry>) -> SessionModelRequest {
        let tools = Arc::new(crate::location::ToolRegistryState::new());
        SessionModelRequest::new(hooks, tools, AppMetadata::default())
    }

    /// A language hook REPLACING the language model changes the
    /// request's target (the transform point, not an observer).
    #[tokio::test]
    async fn language_replacement_changes_request_target() {
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        std::mem::forget(
            hooks
                .register_aisdk_language(
                    fn_hook(|event: &mut crate::hooks::AisdkLanguageEvent| {
                        let route = oc_ai::test_support::echo_route(Arc::new(
                            oc_ai::route::transport::FailingTransport { message: "never sent" },
                        ));
                        let replacement = route
                            .model("replaced-model", Some("parityfake".into()))
                            .expect("replacement model");
                        event.language = Some(replacement);
                        event.options.insert("endpoint".into(), serde_json::json!("responses"));
                        Ok(())
                    }),
                    crate::hooks::HookOptions::default(),
                )
                .await,
        );

        let model = resolved();
        let prepared = service(Arc::clone(&hooks))
            .prepare(RequestKind::Primary, input(&model, vec![Message::user("hello")]))
            .await
            .expect("prepare");
        assert_eq!(
            prepared.request.model.id.0, "replaced-model",
            "the hook's language handle became the request target"
        );
        let options = prepared.request.provider_options.expect("provider options set");
        assert_eq!(
            options.get("endpoint"),
            Some(&serde_json::json!("responses")),
            "the hook's options rode into the provider options (the copilot lane seam)"
        );
    }

    /// Without hooks the resolved model stays the target (the inline
    /// default — copilot.rs's lane selection is unchanged).
    #[tokio::test]
    async fn no_hooks_keeps_resolved_target() {
        let model = resolved();
        let prepared = service(Arc::new(crate::hooks::HookRegistry::new()))
            .prepare(RequestKind::Primary, input(&model, vec![Message::user("hello")]))
            .await
            .expect("prepare");
        assert_eq!(prepared.request.model.id.0, "fake-model");
    }

    /// The sdk event runs FIRST and its handle feeds the language event
    /// (aisdk.ts:346-354's runSDK → runLanguage chaining).
    #[tokio::test]
    async fn sdk_handle_flows_into_language_event() {
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        std::mem::forget(
            hooks
                .register_aisdk_sdk(
                    fn_hook(|event: &mut crate::hooks::AisdkSdkEvent| {
                        event.package = "@ai-sdk/swapped".into();
                        event.sdk = Some(Arc::new(serde_json::json!({ "wrapped": true })));
                        Ok(())
                    }),
                    crate::hooks::HookOptions::default(),
                )
                .await,
        );
        let saw_sdk: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let marker = Arc::clone(&saw_sdk);
        std::mem::forget(
            hooks
                .register_aisdk_language(
                    fn_hook(move |event: &mut crate::hooks::AisdkLanguageEvent| {
                        let has = event
                            .sdk
                            .as_ref()
                            .and_then(|handle| handle.downcast_ref::<serde_json::Value>())
                            .is_some_and(|value| value["wrapped"] == serde_json::json!(true));
                        *marker.lock().unwrap() = has;
                        Ok(())
                    }),
                    crate::hooks::HookOptions::default(),
                )
                .await,
        );

        let model = resolved();
        service(Arc::clone(&hooks))
            .prepare(RequestKind::Primary, input(&model, vec![Message::user("hello")]))
            .await
            .expect("prepare");
        assert!(*saw_sdk.lock().unwrap(), "the sdk event's handle reached the language event");
    }
}

#[cfg(test)]
mod http_interceptor_tests {
    // G-91 — the `session.http.response` event carries the BEFORE-request
    // snapshot (`request: before.request`, model-request.ts:340), i.e. the
    // shaped request the paired request hook produced.
    use super::http_interceptors;
    use crate::hooks::{
        fn_hook, HookOptions, HookRegistry, SessionHttpRequestEvent, SessionHttpResponseEvent,
    };
    use oc_schema::id::{AgentId, SessionId};

    #[tokio::test]
    async fn response_event_carries_before_request_snapshot() {
        let registry = HookRegistry::new();
        let scope = crate::hooks::HookScope::new();

        // The request hook rewrites the URL — the response event must see
        // the SHAPED request, not the raw outbound one.
        let registration = futures::executor::block_on(registry.register_session_http_request(
            fn_hook(|event: &mut SessionHttpRequestEvent| {
                event.request.url = "https://shaped.example/v1/chat".into();
                event.request.headers.insert("x-shaped".into(), "yes".into());
                Ok(())
            }),
            HookOptions::default(),
        ));
        scope.track(registration);

        let seen: std::sync::Arc<std::sync::Mutex<Option<crate::hooks::HttpRequest>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let capture = std::sync::Arc::clone(&seen);
        let registration = futures::executor::block_on(registry.register_session_http_response(
            fn_hook(move |event: &mut SessionHttpResponseEvent| {
                *capture.lock().unwrap() = Some(event.request.clone());
                Ok(())
            }),
            HookOptions::default(),
        ));
        scope.track(registration);

        let intercepts = http_interceptors(
            std::sync::Arc::new(registry),
            SessionId::parse("ses_g91").expect("session id"),
            AgentId::parse("build"),
            oc_schema::model::Ref {
                provider_id: oc_schema::id::ProviderId::parse("parityfake"),
                id: oc_schema::id::ModelId::parse("fake-model"),
                variant: None,
            },
            crate::hooks::SessionRequestKind::Primary,
        );

        let shaped = (intercepts.on_request)(oc_ai::route::transport::TransportRequest {
            method: "POST".into(),
            url: "https://raw.example/v1".into(),
            headers: vec![("x-raw".into(), "1".into())],
            body: Default::default(),
        })
        .await
        .expect("request interceptor");
        assert_eq!(shaped.url, "https://shaped.example/v1/chat");

        let response = (intercepts.on_response)(oc_ai::route::transport::HttpResponseSnapshot {
            status: 200,
            headers: std::collections::BTreeMap::new(),
            body: b"{}".to_vec(),
        })
        .await
        .expect("response interceptor");
        assert_eq!(response.status, 200);

        let request = seen.lock().unwrap().clone().expect("response hook fired");
        assert_eq!(request.url, "https://shaped.example/v1/chat");
        assert_eq!(request.method, "POST");
        assert_eq!(request.headers.get("x-shaped").map(String::as_str), Some("yes"));
    }
}

#[cfg(test)]
mod g193_prompt_surface_tests {
    // G-193.2/G-193.4/G-193.1: the base_transcript prompt surfaces —
    // identity splice at system[1], the optimize family template on the
    // default (non-custom) system prompt, and the Plan-mode reminder
    // reconciliation over the lowered messages.
    use super::base_transcript;
    use crate::session::runner::{plan, ResolvedModel};
    use oc_ai::schema::messages::{ContentPart, Message, MessageRole};
    use oc_schema::session_message::Info as MessageInfo;

    fn resolved_with_info(id: &str, name: &str) -> ResolvedModel {
        let mut resolved = crate::session::runner::tests_harness::test_resolved_model();
        let mut info = oc_schema::model::Info::default_for(
            oc_schema::id::ProviderId::parse("parityfake"),
            oc_schema::id::ModelId::parse(id),
        );
        info.name = name.to_string();
        resolved.info = Some(std::sync::Arc::new(info));
        resolved
    }

    fn user_message(text: &str) -> MessageInfo {
        MessageInfo::User {
            id: oc_schema::id::MessageId::new(),
            metadata: None,
            time: oc_schema::session_message::Time { created: 0 },
            text: text.to_string(),
            files: None,
            agents: None,
            skills: None,
        }
    }

    fn text_of(message: &Message) -> String {
        match message.content.first() {
            Some(ContentPart::Text { text, .. }) => text.clone(),
            _ => String::new(),
        }
    }

    #[test]
    fn identity_section_lands_between_base_prompt_and_initial() {
        let agent = oc_schema::agent::Info::default_for(oc_schema::id::AgentId::parse("build"));
        let model = resolved_with_info("fake-model", "Fake Model");
        let transcript = base_transcript(&agent, &model, &[], "INITIAL INSTRUCTIONS", &[]);
        assert!(transcript.system[0].text.starts_with("You are an AI agent running in OpenCode"));
        assert_eq!(
            transcript.system[1].text,
            "# Your Model\n- Name: Fake Model\n- Provider ID: parityfake\n- Model ID: fake-model"
        );
        assert_eq!(transcript.system[2].text, "INITIAL INSTRUCTIONS");
    }

    #[test]
    fn optimize_family_template_replaces_the_default_prompt() {
        // gpt-family id: the gpt.txt template replaces the default
        // system text; identity still splices at index 1.
        let agent = oc_schema::agent::Info::default_for(oc_schema::id::AgentId::parse("build"));
        let model = resolved_with_info("gpt-5.1", "GPT-5.1");
        let transcript = base_transcript(&agent, &model, &[], "", &[]);
        assert!(transcript.system[0].text.contains("## Final Answer"));
        assert!(transcript.system[1].text.starts_with("# Your Model"));
        // claude-family id: append mode keeps the base text.
        let model = resolved_with_info("claude-sonnet-4", "Claude");
        let transcript = base_transcript(&agent, &model, &[], "", &[]);
        assert!(transcript.system[0].text.starts_with("You are an AI agent running in OpenCode"));
        assert_eq!(
            transcript.system[0].text.trim_end(),
            format!(
                "{}\n\n{}",
                crate::session::runner::system_prompt::make(&[]).trim_end(),
                super::optimize::PROMPT_ANTHROPIC.trim_end()
            )
        );
        // a custom agent system prompt bypasses the family template.
        let mut custom = oc_schema::agent::Info::default_for(oc_schema::id::AgentId::parse("build"));
        custom.system = Some("CUSTOM PROMPT".to_string());
        let transcript = base_transcript(&custom, &model, &[], "", &[]);
        assert_eq!(transcript.system[0].text, "CUSTOM PROMPT");
    }

    #[test]
    fn plan_agent_gets_enter_reminder_others_get_leave() {
        let build = oc_schema::agent::Info::default_for(oc_schema::id::AgentId::parse("build"));
        let model = resolved_with_info("fake-model", "Fake");
        let directory = plan::plan_directory(&oc_util::global_roots::resolve().home);
        let enter = plan::enter_reminder(&directory);

        // plan agent, no reminder yet → enter reminder before the last
        // user message
        let mut plan_agent =
            oc_schema::agent::Info::default_for(oc_schema::id::AgentId::parse("plan"));
        plan_agent.mode = oc_schema::agent::Mode::Primary;
        let transcript =
            base_transcript(&plan_agent, &model, &[], "", &[user_message("plan this")]);
        assert_eq!(transcript.messages.len(), 2);
        assert_eq!(transcript.messages[0].role, MessageRole::User);
        assert_eq!(text_of(&transcript.messages[0]), enter);

        // plan agent already reminded → no double injection
        let transcript = base_transcript(
            &plan_agent,
            &model,
            &[],
            "",
            &[user_message(&enter), user_message("more")],
        );
        assert_eq!(transcript.messages.len(), 2, "no extra reminder: {:?}", transcript
            .messages
            .iter()
            .map(|m| text_of(m))
            .collect::<Vec<_>>());

        // non-plan agent carrying the enter reminder → leave reminder
        let transcript =
            base_transcript(&build, &model, &[], "", &[user_message(&enter), user_message("go")]);
        assert_eq!(transcript.messages.len(), 3);
        assert_eq!(text_of(&transcript.messages[1]), plan::LEAVE_REMINDER);
    }
}
