//! Port of `session/compaction.ts` (§2.5) — the full compaction service:
//! due/ceiling math, conversation split, the summary template/rules, the
//! deliver loop with shrink steps, flatten-and-drop-oldest, immediate
//! usage billing, and the native (provider-side) path.

#![allow(clippy::too_many_lines)]

use std::sync::Arc;

use oc_ai::schema::events::{FinishReason, LLMEvent};
use oc_ai::schema::messages::{Message, MessageRole, ToolEntry};
use oc_ai::schema::ProviderFailureClassification;
use oc_schema::common::SessionError;
use oc_schema::id::{AgentId, MessageId, SessionId};
use oc_schema::model;
use oc_schema::session_message::Info as MessageInfo;
use oc_schema::session_support::ProviderContext;
use serde_json::Value;

use crate::bus::{Bus, PublishOptions};
use crate::database::Database;
use crate::session::runner::context::{
    encode_provider_context, has_measured_prompt, history_load, is_checkpoint, provenance,
    provenance_matches, Boundary, Loaded,
};
use crate::session::runner::model_request::{
    base_transcript, bound_images, unsupported_parts, InputTokens, Prepared, RequestInput,
    SessionModelRequest,
};
use crate::session::runner::retry::RetryPolicy;
use crate::session::runner::{
    content_filter_error, is_retryable, to_session_error, CompactMechanism, LlmClient,
    ModelResolver, RecordedUsage, ResolvedModel, SessionUsage,
};
use futures::StreamExt as _;
use oc_ai::schema::AiError;

/// `Settings` (compaction.ts:41-47).
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub auto: bool,
    /// Tokens kept free below the model's limits before compacting. Unset
    /// keeps 10% free.
    pub buffer: Option<i64>,
    /// Tokens of recent conversation kept verbatim beside the summary.
    pub keep: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { auto: true, buffer: None, keep: 15_000 }
    }
}

/// Why compaction is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Auto,
    Overflow,
    Manual,
}

impl Reason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Auto => "auto",
            Reason::Overflow => "overflow",
            Reason::Manual => "manual",
        }
    }
}

/// The compact trigger (compaction.ts `Trigger`).
pub struct Trigger {
    pub reason: Reason,
    pub context: Loaded,
    /// The `/compact` inbox item id for manual compactions.
    pub input_id: Option<MessageId>,
}

/// The compact outcome (compaction.ts `Outcome`).
#[derive(Debug, Clone)]
pub enum CompactStatus {
    /// Only `auto` skips: the context fits, or automatic compaction is off.
    Skipped,
    Completed,
    Failed {
        error: SessionError,
    },
}

/// A summary fills `text` and `recent`; a native compaction fills
/// `provider_context`.
#[derive(Debug, Clone, Default)]
struct CompactOk {
    text: String,
    recent: String,
    provider_state: Option<oc_schema::common::Record>,
    provider_context: Option<ProviderContext>,
    metadata: Option<oc_schema::common::Record>,
}

type Failure = SessionError;

/// A deliver-loop error: a typed `Failure` never retries; a provider error
/// follows the shrink/retry policy.
enum DeliverError {
    Failure(Failure),
    Provider(AiError),
}

const NOTHING_TO_COMPACT: &str = "Nothing to compact yet";

/// After each "too long" rejection, the next attempt aims at this share of
/// the first rejected request's size.
const SHRINK_STEPS: [f64; 3] = [0.7, 0.5, 0.35];

/// The least of the window kept free for the last reply before compaction
/// and for the summary itself.
const RESERVE_MIN: i64 = 16_000;

/// A common window size, assumed for the compaction request when the
/// model's window is unknown.
const UNKNOWN_WINDOW: i64 = 200_000;

const TOOL_OUTPUT_MAX_CHARS: usize = 1_250;
const IMAGE_TOKEN_ESTIMATE: i64 = 1_500;
const PDF_TOKEN_ESTIMATE: i64 = 2_000;

/// The summary template, verbatim (compaction.ts:104-135).
pub const SUMMARY_TEMPLATE: &str = "You MUST use this format for your response (you may omit sections that aren't applicable). Do not include the <template> tags in your response.\n<template>\n## Objective\n- [one or two brief sentences describing what the user is trying to accomplish]\n\n## Requirements\n- [constraints, preferences, requirements, and scope boundaries stated by the user, or \"(none)\"]\n\n## Decisions\n- [decisions already made and why, or \"(none)\"]\n\n## Work State\nBreak the objective into smaller goals and report which are completed, which are being worked on, and which are blocked.\n### Completed\n- [goals that have been completed; otherwise \"(none)\"]\n\n### Active\n- [goals currently being worked on; otherwise \"(none)\"]\n\n### Blocked\n- [anything blocking progress, and why; otherwise \"(none)\"]\n\n## Next Move\n1. [ordered list of next actions, or \"(none)\"]\n\n## Relevant Files\nList the files and directories, other than the current working directory, that another agent would need to open to continue this work. Include at most 15, most important first. Do not list every file that was read or changed. Include paths outside the current working directory when relevant. If none, write \"(none)\".\n- `[file or directory path]`: [brief reason it matters]\n\n## Important Context\n- [facts the next agent cannot continue without and cannot easily find on its own; or \"(none)\"]\n</template>";

/// The summary rules, verbatim (compaction.ts:137-143).
pub const SUMMARY_RULES: &str = "Rules:\n- Keep each section concise. Use terse, single-line bullets, not prose paragraphs or nested lists.\n- Prefer short references over detailed restatement. It is fine to leave out information the next agent can recover from the code or the files listed above.\n- Preserve exact file paths, symbols, commands, error strings, URLs, and identifiers.\n- Carry forward only user questions or requests that remain unanswered or require further action. Do not repeat ones that newer history has answered or resolved. Preserve exact wording when carrying one forward.\n- Preserve consequential workflow state, including whether changes are uncommitted, committed, pushed, under review, or merged.\n- Do not mention the summary process or that context was compacted.";

/// `buildPrompt` (compaction.ts:145-176).
#[must_use]
pub fn build_prompt(update: bool, legacy: bool) -> String {
    let shared = [
        "Summarize only what the user and the assistant said and did. Leave out instructions and setup the assistant was given rather than told by the user: repository conventions, instruction files such as AGENTS.md, and environment details like the session ID. The next agent receives current versions of all of these separately.".to_string(),
        SUMMARY_TEMPLATE.to_string(),
        SUMMARY_RULES.to_string(),
        "Do not continue the task or call tools.".to_string(),
        "Return only the structured summary in the requested format. Do not include a preamble, explanation, or other commentary.".to_string(),
    ];
    if !update {
        let mut parts =
            vec!["You MUST summarize the conversation above into a structured summary that will be given to another agent to resume the work.".to_string()];
        parts.extend(shared);
        return parts.join("\n\n");
    }
    let mut parts = vec![
        "Update the existing checkpoint in the conversation above into one consolidated summary."
            .to_string(),
    ];
    if legacy {
        parts.push("The existing checkpoint was written with an earlier format that recorded far more detail than this one asks for. Rewrite it at the level of detail described below rather than carrying its detail forward. Keep its requirements, decisions, and open questions; they came from earlier conversation with the user.".to_string());
    }
    parts.push("Newer history always takes precedence over the existing checkpoint. Preserve previous information unless newer history clearly contradicts, supersedes, resolves, or makes it stale. If something is no longer relevant to continuing the work, you may remove it.".to_string());
    parts.push("Incorporate newer requirements, decisions, progress, and context. Reconcile Work State and Next Move: move completed work out of Active, remove resolved blockers and answered questions, and preserve unresolved or pending work.".to_string());
    parts.push("Return only the updated Markdown sections. Do not reproduce the `<conversation-checkpoint>`, `<summary>`, or `<recent-context>` wrapper tags from the previous checkpoint.".to_string());
    parts.extend(shared);
    parts.join("\n\n")
}

/// `NUDGE` (compaction.ts:178-179).
pub const NUDGE: &str = "The previous response did not fill in the required summary template. Do not call tools. Return the summary as text using the exact section headings from the template.";

/// Summaries written with the previous template carry this catch-all
/// heading.
const LEGACY_HEADING: &str = "## Additional Context";

/// The compaction service.
pub struct SessionCompaction {
    db: Database,
    bus: Bus,
    llm: Arc<dyn LlmClient>,
    requests: Arc<SessionModelRequest>,
    agents: Arc<crate::location::AgentRegistry>,
    resolver: Arc<dyn ModelResolver>,
    rng: Arc<dyn oc_util::determinism::RngSource>,
    settings: Arc<parking_lot::RwLock<Settings>>,
    /// Billed usage per compaction run (`spent`, keyed by session).
    spent: Arc<parking_lot::Mutex<std::collections::BTreeMap<String, RecordedUsage>>>,
}

impl SessionCompaction {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        db: Database,
        bus: Bus,
        llm: Arc<dyn LlmClient>,
        requests: Arc<SessionModelRequest>,
        agents: Arc<crate::location::AgentRegistry>,
        resolver: Arc<dyn ModelResolver>,
        rng: Arc<dyn oc_util::determinism::RngSource>,
    ) -> Self {
        Self {
            db,
            bus,
            llm,
            requests,
            agents,
            resolver,
            rng,
            settings: Arc::new(parking_lot::RwLock::new(Settings::default())),
            spent: Arc::new(parking_lot::Mutex::new(std::collections::BTreeMap::new())),
        }
    }

    /// `Editor.configure`.
    pub fn configure(&self, update: Settings) {
        *self.settings.write() = update;
    }

    /// The current settings (CONFIG-C1's consumer proof surface: the
    /// engine's `ConfigCompactionPlugin` wiring is observable through
    /// the configured values).
    #[must_use]
    pub fn settings(&self) -> Settings {
        *self.settings.read()
    }

    /// `compact` (compaction.ts:205-240, 660-673). Manual compactions
    /// settle through their `/compact` inbox item, which the runner owns.
    pub async fn compact(&self, trigger: Trigger) -> CompactStatus {
        let outcome = self.compact_inner(trigger).await;
        self.spent.lock().clear();
        outcome
    }

    async fn compact_inner(&self, trigger: Trigger) -> CompactStatus {
        let settings = *self.settings.read();
        let context = &trigger.context;
        let session_id = context.session.id.clone();

        // Only the user compacts when automatic compaction is off, overflow
        // included.
        if trigger.reason != Reason::Manual && !settings.auto {
            return CompactStatus::Skipped;
        }
        if trigger.reason == Reason::Auto
            && !due(context, calculate_ceiling(&context.model.limit, settings.buffer))
        {
            return CompactStatus::Skipped;
        }
        let native = context.model.compaction == Some(oc_schema::provider::Compaction::Native);
        // The compaction agent's model, else the session's.
        let agent_model = if native {
            None
        } else {
            self.agents.get(&AgentId::parse("compaction")).and_then(|agent| agent.model)
        };
        let model = match agent_model {
            Some(reference) => self
                .resolver
                .resolve_selected(&context.session, &reference)
                .unwrap_or_else(|_| context.model.clone()),
            None => context.model.clone(),
        };
        let ceiling = calculate_ceiling(&model.limit, settings.buffer);
        // An unknown window never triggers auto compaction, but the
        // compaction request still needs a size to aim for.
        let cap = if ceiling.is_finite() {
            ceiling
        } else {
            calculate_ceiling(
                &model::ModelLimit { context: UNKNOWN_WINDOW, ..model.limit },
                settings.buffer,
            )
        };
        // The provider just rejected this context, so the estimate ran low;
        // the first attempt already aims below it.
        let budget = if trigger.reason == Reason::Overflow {
            (cap as i64).min(((estimate_context(context) as f64) * SHRINK_STEPS[0]).floor() as i64)
        } else {
            cap as i64
        };

        let mut selected = trigger.context.clone();
        selected.model = model.clone();
        let result = if native {
            self.compact_natively(&selected, budget, settings.keep).await
        } else {
            self.summarize(trigger.reason, &selected, budget, settings.keep).await
        };
        match result {
            Ok(result) => {
                self.publish(&session_id, trigger.reason, trigger.input_id, &model, Ok(result))
                    .await
            }
            Err(error) => {
                self.publish(&session_id, trigger.reason, trigger.input_id, &model, Err(error))
                    .await
            }
        }
    }

    // -----------------------------------------------------------------------
    // Summarize path
    // -----------------------------------------------------------------------

    /// `summarize` (compaction.ts:272-310).
    async fn summarize(
        &self,
        reason: Reason,
        context: &Loaded,
        budget: i64,
        keep: i64,
    ) -> Result<CompactOk, DeliverError> {
        let Some(split) = split_conversation(&context.messages, keep) else {
            return Err(DeliverError::Failure(compaction_unavailable()));
        };

        let previous = previous_compaction(&context.messages);
        let prompt = build_prompt(
            previous.is_some(),
            previous
                .map(|message| match message {
                    MessageInfo::Compaction(
                        oc_schema::session_message::Compaction::Completed { summary, .. },
                    ) => summary.contains(LEGACY_HEADING),
                    _ => false,
                })
                .unwrap_or(false),
        );
        let headings: Vec<&str> =
            SUMMARY_TEMPLATE.lines().filter(|line| line.starts_with("##")).collect();
        let filled = |text: &str| text.lines().any(|line| headings.contains(&line.trim()));
        let prepared = self.prepare(context, &split.0, budget).await?;

        // Hooks saw the request without the summary prompt, so it is
        // appended here. A reply that ignores the template gets one
        // reminder before it counts as a failure.
        let send = |mut request: oc_ai::schema::messages::LLMRequest| {
            let prompt = prompt.clone();
            let recent = split.1.clone();
            async move {
                request.messages.push(Message::user(prompt.clone()));
                let reply = stream_summary(self, context, request.clone()).await?;
                if filled(&reply.text) {
                    return Ok(CompactOk {
                        text: reply.text,
                        recent,
                        provider_state: reply.provider_state,
                        ..Default::default()
                    });
                }
                request.messages.push(Message::user(NUDGE));
                let retry = stream_summary(self, context, request).await?;
                if filled(&retry.text) {
                    return Ok(CompactOk {
                        text: retry.text,
                        recent,
                        provider_state: retry.provider_state,
                        ..Default::default()
                    });
                }
                Err(DeliverError::Failure(SessionError {
                    error_type: "compaction.failed".into(),
                    message: if retry.text.trim().is_empty() {
                        "Compaction produced no summary".to_string()
                    } else {
                        "Compaction summary did not match the required template".to_string()
                    },
                    status: None,
                    response: None,
                }))
            }
        };

        let overhead = token_estimate(&prompt) + token_estimate(NUDGE);
        self.deliver(reason, context, prepared, &split.1, budget - overhead, send).await
    }

    // -----------------------------------------------------------------------
    // Native path (compaction.ts:324-372)
    // -----------------------------------------------------------------------

    async fn compact_natively(
        &self,
        context: &Loaded,
        budget: i64,
        keep: i64,
    ) -> Result<CompactOk, DeliverError> {
        if !context.messages.iter().any(|message| !message_to_text(message).is_empty()) {
            return Err(DeliverError::Failure(compaction_unavailable()));
        }
        let unsupported = |message: &str| {
            Err::<CompactOk, DeliverError>(DeliverError::Failure(SessionError {
                error_type: "provider.unsupported-operation".into(),
                message: message.to_string(),
                status: None,
                response: None,
            }))
        };
        let prepared = self.prepare(context, &context.messages, budget).await?;

        // History is selected before request hooks, so a hook that reroutes
        // the request cannot be honored here.
        let Some(provenance_value) = provenance(&context.model) else {
            return unsupported("Provider compaction requires a stable, configured endpoint");
        };
        let routed = routed_provenance(&prepared, &context.model);
        if !provenance_matches(&provenance_value, routed.as_ref()) {
            return unsupported(
                "Provider compaction requires the endpoint in provider/model settings, not a model.request rewrite",
            );
        }

        let send = |request: oc_ai::schema::messages::LLMRequest| {
            let provenance_value = provenance_value.clone();
            async move {
                if self.llm.can_compact(&request, CompactMechanism::Trigger) {
                    let history =
                        history_load(&self.db, context.session.id.as_str(), &Boundary::Local)
                            .await
                            .unwrap_or_default();
                    let retained = recent_user_messages(&history, &context.model, keep);
                    let outcome = self
                        .llm
                        .compact(request, CompactMechanism::Trigger)
                        .await
                        .map_err(DeliverError::Provider)?;
                    self.spend(
                        &context.session.id,
                        Some(SessionUsage::record(outcome.usage.as_ref(), &context.model.cost)),
                    )
                    .await;
                    let mut window = retained;
                    if let Some(checkpoint) = outcome.checkpoint {
                        window.push(Message::assistant(vec![checkpoint]));
                    }
                    return Ok(CompactOk {
                        text: String::new(),
                        recent: String::new(),
                        provider_context: Some(encode_provider_context(provenance_value, &window)),
                        ..Default::default()
                    });
                }
                if self.llm.can_compact(&request, CompactMechanism::Endpoint) {
                    let outcome = self
                        .llm
                        .compact(request, CompactMechanism::Endpoint)
                        .await
                        .map_err(DeliverError::Provider)?;
                    self.spend(
                        &context.session.id,
                        Some(SessionUsage::record(outcome.usage.as_ref(), &context.model.cost)),
                    )
                    .await;
                    return Ok(CompactOk {
                        text: String::new(),
                        recent: String::new(),
                        provider_context: Some(encode_provider_context(
                            provenance_value,
                            &outcome.replacement,
                        )),
                        ..Default::default()
                    });
                }
                unsupported(&format!(
                    "Native compaction is not supported for {}/{}",
                    request.model.provider.0, request.model.route.id,
                ))
            }
        };

        self.deliver(Reason::Auto, context, prepared, "", budget, send).await
    }

    // -----------------------------------------------------------------------
    // Deliver loop (compaction.ts:384-455)
    // -----------------------------------------------------------------------

    async fn deliver<F, Fut>(
        &self,
        reason: Reason,
        context: &Loaded,
        prepared: Prepared,
        recent: &str,
        mut target: i64,
        send: F,
    ) -> Result<CompactOk, DeliverError>
    where
        F: Fn(oc_ai::schema::messages::LLMRequest) -> Fut,
        Fut: std::future::Future<Output = Result<CompactOk, DeliverError>>,
    {
        // The runner opened the manual compaction's message when it
        // delivered the `/compact` item.
        if reason != Reason::Manual {
            let data = serde_json::json!({
                "sessionID": context.session.id,
                "reason": "auto",
                "recent": recent,
            });
            let def = crate::bus::registry::by_type_latest("session.compaction.started")
                .unwrap_or_else(|| panic!("no session.compaction.started definition"));
            let _ = self.bus.publish(def, data, PublishOptions::default()).await;
        }
        // TODO(wave-C): the `session.compaction` hook short-circuit result
        // (`prepared.event.result`).

        let policy = RetryPolicy::new(
            SessionId::parse(context.session.id.clone()).unwrap_or_else(|err| SessionId(err.value)),
            Arc::clone(&self.rng),
        );
        let mut rejections = 0usize;
        let mut rejected: Option<i64> = None;
        let mut as_text = false;

        loop {
            let fits = !as_text && estimate_request_of(&prepared.request) <= target;
            // `request === prepared.request` (compaction.ts:417-424): the
            // request identity the free as-text resend keys on. Rust clones
            // instead of sharing, so the identity travels as a flag on the
            // branches that hand out an unchanged clone of the original;
            // every flattened build is a new request, never the original.
            let (request, is_original) = if fits {
                (Some(prepared.request.clone()), true)
            } else {
                match flatten_and_drop_oldest(&prepared.request, context, target) {
                    Some(flattened) => (Some(flattened), false),
                    None => {
                        if rejections > 0 {
                            (None, false)
                        } else if as_text {
                            (flatten_and_drop_oldest(&prepared.request, context, i64::MAX), false)
                        } else {
                            (Some(prepared.request.clone()), true)
                        }
                    }
                }
            };
            let Some(request) = request else {
                return Err(DeliverError::Failure(SessionError {
                    error_type: "compaction.failed".into(),
                    message: "The compaction request cannot be reduced further without losing the latest exchange or checkpoint".to_string(),
                    status: None,
                    response: None,
                }));
            };

            let attempt = send(request.clone()).await;
            match attempt {
                Ok(result) => return Ok(result),
                Err(DeliverError::Failure(failure)) => return Err(DeliverError::Failure(failure)),
                Err(DeliverError::Provider(cause)) => {
                    let too_large = is_payload_too_large(&cause);
                    if too_large && is_original {
                        // A "payload too large" rejection is about bytes;
                        // resend the unchanged request as text, uncounted
                        // (the first 413 never buys a shrink rung).
                        as_text = true;
                        continue;
                    }
                    if too_large || crate::session::runner::is_context_overflow_failure(&cause) {
                        // The estimate ran low: the next attempts aim at
                        // 70%, 50%, then 35% of the first rejected request.
                        let Some(step) = SHRINK_STEPS.get(rejections) else {
                            return Err(DeliverError::Failure(to_session_error(&cause)));
                        };
                        rejected.get_or_insert_with(|| estimate_request_of(&request));
                        target = ((rejected.unwrap_or(0)) as f64 * step).floor() as i64;
                        rejections += 1;
                        continue;
                    }
                    // Other provider errors resend the same request under
                    // the session's retry policy.
                    let error = to_session_error(&cause);
                    let decision = policy
                        .decide(
                            &cause,
                            &error,
                            &context.agent_id,
                            &context.model.reference,
                            is_retryable(&cause),
                            None,
                        )
                        .await;
                    if !decision.retry {
                        return Err(DeliverError::Failure(error));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(decision.delay_ms)).await;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Preparation / billing / publication
    // -----------------------------------------------------------------------

    /// `prepare` (compaction.ts:575-592): the conversation as the runner
    /// would send it, after request hooks.
    async fn prepare(
        &self,
        context: &Loaded,
        messages: &[MessageInfo],
        budget: i64,
    ) -> Result<Prepared, DeliverError> {
        let base = base_transcript(
            &context.agent,
            &context.model,
            &context.tools,
            &context.initial,
            messages,
        );
        self.requests
            .compaction(RequestInput {
                session: context.session.clone(),
                agent: context.agent_id.clone(),
                model: context.model.clone(),
                tools: Some(context.tools.clone()),
                system: base.system,
                messages: base.messages,
                tool_choice: None,
                web_socket: false,
                input_tokens: Some(InputTokens { measured: budget as f64, estimated: 0.0 }),
            })
            .await
            .map_err(|error| {
                DeliverError::Failure(SessionError {
                    error_type: "provider.no-route".into(),
                    message: error.to_string(),
                    status: None,
                    response: None,
                })
            })
    }

    /// Each model call is billed as soon as it finishes (`spend`).
    async fn spend(&self, session_id: &str, usage: Option<RecordedUsage>) {
        let Some(usage) = usage else { return };
        let total = {
            let mut spent = self.spent.lock();
            let entry = spent.entry(session_id.to_string()).or_default();
            let total = SessionUsage::add(entry, &usage);
            *entry = total.clone();
            total
        };
        let data = serde_json::json!({
            "sessionID": session_id,
            "source": "compaction",
            "cost": total.cost.0 .0,
            "tokens": crate::session::runner::tokens_json(&total.tokens),
        });
        let def = crate::bus::registry::by_type_latest("session.usage.recorded")
            .unwrap_or_else(|| panic!("no session.usage.recorded definition"));
        let _ = self.bus.publish(def, data, PublishOptions::default()).await;
    }

    /// `publish` (compaction.ts:623-658).
    async fn publish(
        &self,
        session_id: &str,
        reason: Reason,
        input_id: Option<MessageId>,
        model: &ResolvedModel,
        outcome: Result<CompactOk, DeliverError>,
    ) -> CompactStatus {
        let usage = self.spent.lock().get(session_id).cloned();
        match outcome {
            Err(error) => {
                let error = match error {
                    DeliverError::Failure(failure) => failure,
                    DeliverError::Provider(cause) => to_session_error(&cause),
                };
                let mut data = serde_json::json!({
                    "sessionID": session_id,
                    "reason": reason.as_str(),
                    "error": serde_json::to_value(&error).unwrap_or_default(),
                });
                if let Some(input_id) = &input_id {
                    data["inputID"] = serde_json::json!(input_id.as_str());
                }
                if let Some(usage) = usage {
                    data["cost"] = serde_json::json!(usage.cost.0 .0);
                    data["tokens"] = crate::session::runner::tokens_json(&usage.tokens);
                }
                let def = crate::bus::registry::by_type_latest("session.compaction.failed")
                    .unwrap_or_else(|| panic!("no session.compaction.failed definition"));
                let _ = self.bus.publish(def, data, PublishOptions::default()).await;
                CompactStatus::Failed { error }
            }
            Ok(result) => {
                let mut data = serde_json::json!({
                    "sessionID": session_id,
                    "reason": reason.as_str(),
                    "model": serde_json::to_value(&model.reference).unwrap_or_default(),
                    "text": result.text,
                    "recent": result.recent,
                });
                if let Some(state) = &result.provider_state {
                    data["providerState"] =
                        serde_json::Value::Object(state.clone().into_iter().collect());
                }
                if let Some(context) = &result.provider_context {
                    data["providerContext"] = serde_json::to_value(context).unwrap_or_default();
                }
                if let Some(usage) = usage {
                    data["cost"] = serde_json::json!(usage.cost.0 .0);
                    data["tokens"] = crate::session::runner::tokens_json(&usage.tokens);
                }
                let mut options = PublishOptions::default();
                if let Some(metadata) = &result.metadata {
                    options.metadata =
                        Some(serde_json::Value::Object(metadata.clone().into_iter().collect()));
                }
                let def = crate::bus::registry::by_type_latest("session.compaction.ended")
                    .unwrap_or_else(|| panic!("no session.compaction.ended definition"));
                let _ = self.bus.publish(def, data, options).await;
                CompactStatus::Completed
            }
        }
    }
}

/// The summary stream fold (compaction.ts:506-565).
async fn stream_summary(
    service: &SessionCompaction,
    context: &Loaded,
    request: oc_ai::schema::messages::LLMRequest,
) -> Result<Streamed, DeliverError> {
    let session_id = context.session.id.as_str();
    let metadata_key = context.model.provider_metadata_key().to_string();
    let mut stream = service.llm.stream(request).await.map_err(DeliverError::Provider)?;
    let mut streamed = Streamed::default();
    while let Some(event) = stream.next().await {
        let event = match event {
            Ok(event) => event,
            Err(error) => return Err(DeliverError::Provider(error)),
        };
        match &event {
            LLMEvent::ProviderError { message, classification: Some(classification), .. } => {
                // A classified provider error retries through the deliver
                // loop's policy.
                return Err(DeliverError::Provider(classified_invalid_request(
                    message,
                    classification,
                )));
            }
            LLMEvent::ProviderError { message, .. } => {
                return Err(DeliverError::Failure(SessionError {
                    error_type: "provider.error".into(),
                    message: message.clone(),
                    status: None,
                    response: None,
                }));
            }
            LLMEvent::TextDelta { text, .. } => {
                streamed.text.push_str(text);
                let data = serde_json::json!({ "sessionID": session_id, "text": text });
                let def = crate::bus::registry::by_type_latest("session.compaction.delta")
                    .unwrap_or_else(|| panic!("no session.compaction.delta definition"));
                let _ = service.bus.publish(def, data, PublishOptions::default()).await;
            }
            LLMEvent::StepFinish { usage, provider_metadata, .. } => {
                let recorded = SessionUsage::record(usage.as_ref(), &context.model.cost);
                service.spend(&context.session.id, Some(recorded)).await;
                streamed.provider_state = provider_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get(&metadata_key))
                    .cloned()
                    .map(|object| object.into_iter().collect());
            }
            LLMEvent::Finish { reason, .. } => match reason.normalized {
                FinishReason::Unknown => {
                    return Err(DeliverError::Provider(
                        crate::session::runner::unknown_finish_error(),
                    ));
                }
                FinishReason::Error => {
                    return Err(DeliverError::Failure(SessionError {
                        error_type: "provider.unknown".into(),
                        message: "Compaction generation failed".into(),
                        status: None,
                        response: None,
                    }));
                }
                FinishReason::Length => {
                    return Err(DeliverError::Failure(SessionError {
                        error_type: "compaction.failed".into(),
                        message: "Compaction summary reached the output token limit".into(),
                        status: None,
                        response: None,
                    }));
                }
                FinishReason::ContentFilter => {
                    return Err(DeliverError::Failure(content_filter_error(
                        "Compaction summary was blocked by the provider",
                        reason,
                    )));
                }
                FinishReason::Stop | FinishReason::ToolCalls => {}
            },
            _ => {}
        }
    }
    Ok(streamed)
}

#[derive(Debug, Clone, Default)]
struct Streamed {
    text: String,
    provider_state: Option<oc_schema::common::Record>,
}

fn compaction_unavailable() -> Failure {
    SessionError {
        error_type: "compaction.unavailable".into(),
        message: NOTHING_TO_COMPACT.to_string(),
        status: None,
        response: None,
    }
}

/// A classified invalid-request error (the deliver loop's shrink path).
fn classified_invalid_request(
    message: &str,
    classification_: &ProviderFailureClassification,
) -> AiError {
    let mut error = oc_ai::schema::AiError::invalid_request(message);
    if let oc_ai::schema::AiErrorReason::InvalidRequest { classification: slot, .. } =
        &mut error.reason
    {
        *slot = Some(*classification_);
    }
    error
}

fn is_payload_too_large(error: &AiError) -> bool {
    matches!(
        &error.reason,
        oc_ai::schema::AiErrorReason::InvalidRequest {
            classification: Some(ProviderFailureClassification::PayloadTooLarge),
            ..
        }
    )
}

/// The routed provenance of a prepared request (after hooks).
fn routed_provenance(
    prepared: &Prepared,
    model: &ResolvedModel,
) -> Option<oc_schema::session_support::Provenance> {
    let routed = ResolvedModel {
        model: prepared.request.model.clone(),
        reference: model.reference.clone(),
        capabilities: model.capabilities.clone(),
        cost: model.cost.clone(),
        limit: model.limit,
        compaction: model.compaction,
        transport: model.transport,
    };
    provenance(&routed)
}

/// `due` (compaction.ts:242-251).
#[must_use]
pub fn due(context: &Loaded, ceiling: f64) -> bool {
    let messages = &context.messages;
    // A compaction just completed; let the runner rebuild the request from
    // it first.
    if let Some(last) = messages.last() {
        if matches!(
            last,
            MessageInfo::Compaction(oc_schema::session_message::Compaction::Completed { .. })
        ) {
            return false;
        }
    }
    // An encrypted native window estimates as nothing, so wait for a
    // response to measure it.
    let measured =
        messages.iter().rposition(|message| has_measured_prompt(message, &context.model.reference));
    let checkpoint = messages.iter().rposition(is_checkpoint);
    if let (Some(measured), Some(checkpoint)) = (measured, checkpoint) {
        if measured < checkpoint {
            return false;
        }
    }
    (estimate_context(context) as f64) >= ceiling
}

/// `calculateCeiling` (compaction.ts:905-912).
#[must_use]
pub fn calculate_ceiling(limit: &model::ModelLimit, buffer: Option<i64>) -> f64 {
    // Unknown limits are reported as 0. An unknown input limit falls back
    // to the context window (`limit.input || limit.context`).
    let window = match limit.input {
        Some(input) if input > 0 => input,
        _ => limit.context,
    };
    if window <= 0 {
        return f64::INFINITY;
    }
    if let Some(buffer) = buffer {
        return (window - buffer) as f64;
    }
    let reserve = if window >= 2 * RESERVE_MIN { RESERVE_MIN } else { 0 };
    let floor = ((window as f64) * 0.1).floor() as i64;
    (window - floor.max(reserve)) as f64
}

/// `Token.estimate` (util/token.ts): ~4 chars per token.
#[must_use]
pub fn token_estimate(input: &str) -> i64 {
    ((input.len() as f64) / 4.0).round() as i64
}

/// `estimateContext` (compaction.ts:868-871).
#[must_use]
pub fn estimate_context(context: &Loaded) -> i64 {
    let (measured, estimated) = estimate_prompt(context);
    measured + estimated
}

/// `estimatePrompt` (compaction.ts:874-898): `measured` is what the
/// provider reported at the latest response, `estimated` the text since.
#[must_use]
pub fn estimate_prompt(context: &Loaded) -> (i64, i64) {
    let anchor_index = context
        .messages
        .iter()
        .rposition(|message| has_measured_prompt(message, &context.model.reference));
    let anchor = anchor_index.map(|index| &context.messages[index]);
    let base = base_transcript(
        &context.agent,
        &context.model,
        &context.tools,
        &context.initial,
        &context.messages[anchor_index.unwrap_or(0)..],
    );
    let sent = bound_images(unsupported_parts(base.messages, &context.model.capabilities));
    // The anchor's usage covers its own output, but not its local tool
    // results, which the provider never saw.
    let unmeasured: Vec<&Message> = sent
        .iter()
        .filter(|message| {
            let anchor_id = anchor.and_then(|anchor| match anchor {
                MessageInfo::Assistant { id, .. } => Some(id.to_string()),
                _ => None,
            });
            message.role != MessageRole::Assistant || message.id != anchor_id
        })
        .collect();

    let Some(MessageInfo::Assistant { tokens: Some(tokens), .. }) = anchor else {
        let system: Vec<String> = base.system.iter().map(|part| part.text.clone()).collect();
        let tools: Vec<ToolEntry> = context.tools.iter().map(tool_entry_of).collect();
        return (0, estimate_request_parts(&system, &tools, &unmeasured));
    };
    let measured = tokens.input.0
        + tokens.cache.read.0
        + tokens.cache.write.0
        + tokens.output.0
        + tokens.reasoning.0;
    (measured as i64, unmeasured.iter().map(|message| estimate_message(message)).sum())
}

fn tool_entry_of(tool: &crate::location::ToolRegistration) -> ToolEntry {
    ToolEntry::Definition(oc_ai::schema::messages::ToolDefinition::new(
        tool.name.clone(),
        tool.description.clone(),
        tool.input.clone(),
    ))
}

/// `estimateRequest` (compaction.ts:925-928).
#[must_use]
pub fn estimate_request_of(request: &oc_ai::schema::messages::LLMRequest) -> i64 {
    let system: Vec<String> = request.system.iter().map(|part| part.text.clone()).collect();
    let messages: Vec<&Message> = request.messages.iter().collect();
    estimate_request_parts(&system, &request.tools, &messages)
}

fn estimate_request_parts(system: &[String], tools: &[ToolEntry], messages: &[&Message]) -> i64 {
    system.iter().map(|text| token_estimate(text)).sum::<i64>()
        + tools.iter().map(estimate_tool).sum::<i64>()
        + messages.iter().map(|message| estimate_message(message)).sum::<i64>()
}

/// `estimateTool` (compaction.ts:931-937): only what providers receive.
fn estimate_tool(tool: &ToolEntry) -> i64 {
    use oc_ai::schema::messages::ToolEntry as Entry;
    match tool {
        Entry::Definition(tool) => token_estimate(&format!(
            "{}{}{}",
            tool.name,
            tool.description,
            serde_json::to_string(&tool.input_schema).unwrap_or_default()
        )),
        Entry::Namespace(namespace) => {
            token_estimate(&format!(
                "{}{}",
                namespace.name,
                namespace.description.clone().unwrap_or_default()
            )) + namespace.tools.iter().map(estimate_tool).sum::<i64>()
        }
    }
}

/// `estimateMessage` (compaction.ts:939).
fn estimate_message(message: &Message) -> i64 {
    message.content.iter().map(estimate_part).sum()
}

/// `estimatePart` (compaction.ts:941-956).
fn estimate_part(part: &oc_ai::schema::messages::ContentPart) -> i64 {
    use oc_ai::schema::messages::{ContentPart as Part, ToolResultValue};
    match part {
        Part::Text { text, .. } | Part::Reasoning { text, .. } => token_estimate(text),
        Part::Media { media, .. } => estimate_media(&media.media_type),
        Part::ToolCall { name, input, .. } => {
            token_estimate(&format!("{name}{}", serde_json::to_string(input).unwrap_or_default()))
        }
        Part::ToolResult { result, .. } => match result {
            ToolResultValue::Content { value } => value
                .0
                .iter()
                .map(|item| {
                    if item.get("type").and_then(Value::as_str) == Some("text") {
                        token_estimate(item.get("text").and_then(Value::as_str).unwrap_or_default())
                    } else {
                        estimate_media(item.get("mime").and_then(Value::as_str).unwrap_or_default())
                    }
                })
                .sum(),
            ToolResultValue::Json { value }
            | ToolResultValue::Text { value }
            | ToolResultValue::Error { value } => token_estimate(&stringify_value(value)),
        },
        Part::Compaction { text, .. } => token_estimate(text.as_deref().unwrap_or("")),
        Part::Effort { .. } => 0,
    }
}

fn stringify_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// `estimateMedia` (compaction.ts:958-963).
fn estimate_media(mime: &str) -> i64 {
    let mime = mime.to_ascii_lowercase();
    if mime.starts_with("image/") {
        IMAGE_TOKEN_ESTIMATE
    } else if mime == "application/pdf" {
        PDF_TOKEN_ESTIMATE
    } else {
        0
    }
}

/// `previousCompaction` (compaction.ts:684-687): history loads from the
/// latest completed compaction, so a previous one is always the first
/// message.
#[must_use]
pub fn previous_compaction(messages: &[MessageInfo]) -> Option<&MessageInfo> {
    match messages.first() {
        Some(
            message @ MessageInfo::Compaction(oc_schema::session_message::Compaction::Completed {
                ..
            }),
        ) => Some(message),
        _ => None,
    }
}

/// `splitConversation` (compaction.ts:702-714): `older` gets summarized;
/// `recent`, the newest messages within `keep` tokens, is kept verbatim.
#[must_use]
pub fn split_conversation(
    messages: &[MessageInfo],
    keep: i64,
) -> Option<(Vec<MessageInfo>, String)> {
    let entries: Vec<(usize, String)> = messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| {
            let text = message_to_text(message);
            if text.is_empty() {
                None
            } else {
                Some((index, text))
            }
        })
        .collect();
    if entries.is_empty() {
        return None;
    }
    let start = recent_start(&entries, messages, keep);
    let older_end = entries.get(start).map_or(messages.len(), |(index, _)| *index);
    let older: Vec<MessageInfo> = messages[..older_end].to_vec();
    let recent: Vec<String> =
        entries[start.min(entries.len())..].iter().map(|(_, text)| text.clone()).collect();
    Some((older, recent.join("\n\n")))
}

/// `recentStart` (compaction.ts:716-738).
fn recent_start(entries: &[(usize, String)], messages: &[MessageInfo], keep: i64) -> usize {
    // Drop the oldest entries until the rest fit the allowance, but always
    // keep the newest one.
    let sizes: Vec<i64> = entries.iter().map(|(_, text)| token_estimate(text)).collect();
    let dropped = oldest_to_drop(&sizes, keep).min(entries.len().saturating_sub(1));

    // Start at a user message so an assistant's tool calls and results stay
    // together.
    let user_boundary = (0..=dropped.min(entries.len().saturating_sub(1)))
        .rev()
        .find(|index| matches!(messages[entries[*index].0], MessageInfo::User { .. }));
    if let Some(position) = user_boundary {
        if position > 0 {
            return position;
        }
    }

    // Everything fits. Keep only the latest exchange so there is an older
    // part left to summarize.
    let latest_user =
        entries.iter().rposition(|(index, _)| matches!(messages[*index], MessageInfo::User { .. }));
    if let Some(position) = latest_user {
        if position > 0 {
            return position;
        }
    }

    // One exchange, nothing older. Summarize it all and keep nothing,
    // unless a previous summary already kept recent text.
    let has_previous_recent = previous_compaction(messages)
        .map(|message| match message {
            MessageInfo::Compaction(oc_schema::session_message::Compaction::Completed {
                recent,
                ..
            }) => !recent.is_empty(),
            _ => false,
        })
        .unwrap_or(false);
    if has_previous_recent {
        0
    } else {
        entries.len()
    }
}

/// `oldestToDrop` (compaction.ts:740-750).
#[must_use]
pub fn oldest_to_drop(sizes: &[i64], budget: i64) -> usize {
    let mut total = 0i64;
    let mut start = sizes.len();
    while start > 0 {
        let next = total + sizes[start - 1];
        if next > budget {
            break;
        }
        total = next;
        start -= 1;
    }
    start
}

/// `messageToText` (compaction.ts:753-805): one message as the recent,
/// verbatim part of a summary shows it.
#[must_use]
pub fn message_to_text(message: &MessageInfo) -> String {
    use oc_schema::prompt::FileSource;
    use oc_schema::session_message::{AssistantContent, ToolState};
    match message {
        MessageInfo::Compaction(_) | MessageInfo::System { .. } => String::new(),
        MessageInfo::User { text, skills, files, .. } => {
            let mut lines: Vec<String> = Vec::new();
            if let Some(skills) = skills {
                for skill in skills {
                    if let Some(skill_text) = &skill.text {
                        lines.push(format!("[Skill activated: {}]\n{skill_text}", skill.name.0));
                    }
                }
            }
            lines.push(format!("[User]: {text}"));
            if let Some(files) = files {
                for file in files {
                    let name = file.name.clone().unwrap_or_else(|| match &file.source {
                        FileSource::Uri { uri } => uri.clone(),
                        FileSource::Inline => "inline attachment".to_string(),
                    });
                    lines.push(format!("[Attached {}: {name}]", file.mime));
                }
            }
            lines.join("\n")
        }
        MessageInfo::LocationSwitched { location, .. } => {
            format!("[User]: The working directory has been changed to {}.", location.directory.0)
        }
        MessageInfo::Assistant { content, .. } => content
            .iter()
            .flat_map(|part| -> Vec<String> {
                match part {
                    AssistantContent::Text { text, .. } => vec![format!("[Assistant]: {text}")],
                    AssistantContent::Reasoning { text, .. } => {
                        if text.is_empty() {
                            Vec::new()
                        } else {
                            vec![format!("[Assistant reasoning]: {text}")]
                        }
                    }
                    AssistantContent::Tool { name, state, .. } => {
                        let input = match state {
                            ToolState::Streaming { input } => input.clone(),
                            ToolState::Running { input, .. }
                            | ToolState::Completed { input, .. }
                            | ToolState::Error { input, .. } => serde_json::to_string(
                                &serde_json::Value::Object(input.clone().into_iter().collect()),
                            )
                            .unwrap_or_default(),
                        };
                        let call = format!("[Assistant tool call]: {name}({input})");
                        match state {
                            ToolState::Completed { content, .. } => vec![
                                call,
                                format!(
                                    "[Tool result]: {}",
                                    truncate_tool_output(&serialize_tool_content(content))
                                ),
                            ],
                            ToolState::Error { error, .. } => {
                                vec![call, format!("[Tool error]: {}", error.message)]
                            }
                            _ => vec![call],
                        }
                    }
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        MessageInfo::Synthetic { text, .. } => format!("[Synthetic context]: {text}"),
        MessageInfo::Skill { name, text, .. } => format!("[Skill activated: {}]\n{text}", name.0),
        MessageInfo::Shell { command, output, metadata, .. } => {
            let background = metadata
                .as_ref()
                .and_then(|metadata| metadata.get("background"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if background {
                return String::new();
            }
            format!(
                "[Shell]: {command}\n{}",
                truncate_tool_output(
                    output.as_ref().map(|output| output.output.as_str()).unwrap_or_default()
                )
            )
        }
        _ => String::new(),
    }
}

/// `truncateToolOutput` (compaction.ts:807-820).
#[must_use]
pub fn truncate_tool_output(value: &str) -> String {
    if value.chars().count() <= TOOL_OUTPUT_MAX_CHARS {
        return value.to_string();
    }
    let kept: String = value.chars().take(TOOL_OUTPUT_MAX_CHARS).collect();
    format!("{kept}\n[truncated]")
}

/// `serializeToolContent` (compaction.ts:845-851).
fn serialize_tool_content(content: &[oc_schema::tool::Content]) -> String {
    content
        .iter()
        .map(|item| match item {
            oc_schema::tool::Content::Text { text } => text.clone(),
            oc_schema::tool::Content::File { mime, name, .. } => match name {
                Some(name) if !name.is_empty() => format!("[Attached {mime}: {name}]"),
                _ => format!("[Attached {mime}]"),
            },
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `flattenAndDropOldest` (compaction.ts:465-504): history that has to
/// change is sent as text — `[previous compaction][transcript]` with whole
/// exchanges dropped oldest-first. `None` when even the newest exchange
/// does not fit.
#[must_use]
pub fn flatten_and_drop_oldest(
    request: &oc_ai::schema::messages::LLMRequest,
    context: &Loaded,
    target: i64,
) -> Option<oc_ai::schema::messages::LLMRequest> {
    use oc_ai::schema::messages::ContentPart;
    let previous = previous_compaction(&context.messages);
    let previous_id = previous.and_then(|previous| match previous {
        MessageInfo::Compaction(oc_schema::session_message::Compaction::Completed {
            id, ..
        }) => Some(id.to_string()),
        _ => None,
    });
    let end = request
        .messages
        .iter()
        .rposition(|message| {
            previous_id.as_deref().is_some_and(|id| message.id.as_deref() == Some(id))
                || message.content.iter().any(|part| matches!(part, ContentPart::Compaction { .. }))
        })
        .map_or(0, |index| index + 1);
    let summary = previous.filter(|message| !is_checkpoint(message));
    let lead: Vec<Message> = match summary {
        Some(summary) => crate::session::runner::to_llm_message::to_llm_messages(
            &[strip_recent(summary)],
            &context.model.reference,
            context.model.provider_metadata_key(),
        ),
        None => request.messages[..end].to_vec(),
    };

    // A user message and everything after it, up to the next one, is one
    // exchange: kept or dropped whole.
    let mut exchanges: Vec<String> = Vec::new();
    if let Some(MessageInfo::Compaction(oc_schema::session_message::Compaction::Completed {
        recent,
        ..
    })) = summary
    {
        if !recent.is_empty() {
            exchanges.push(recent.clone());
        }
    }
    for message in &request.messages[end.min(request.messages.len())..] {
        let text = flatten_message(message);
        if text.is_empty() {
            continue;
        }
        if message.role == MessageRole::User || exchanges.is_empty() {
            exchanges.push(text);
        } else if let Some(last) = exchanges.last_mut() {
            last.push_str(&format!("\n{text}"));
        }
    }

    let note = |omitted: usize| {
        if omitted == 0 {
            String::new()
        } else {
            format!(
                "[{omitted} older {} omitted]\n\n",
                if omitted == 1 { "exchange" } else { "exchanges" }
            )
        }
    };
    // Room for the longest possible note is set aside before choosing what
    // to keep.
    let sizes: Vec<i64> = exchanges.iter().map(|text| token_estimate(text) + 1).collect();
    let system: Vec<String> = request.system.iter().map(|part| part.text.clone()).collect();
    let lead_refs: Vec<&Message> = lead.iter().collect();
    let room = target
        - estimate_request_parts(&system, &request.tools, &lead_refs)
        - token_estimate(&note(exchanges.len()));
    let kept_from = oldest_to_drop(&sizes, room);
    let kept = &exchanges[kept_from.min(exchanges.len())..];
    if kept.is_empty() {
        return None;
    }
    let mut next = request.clone();
    next.messages = lead;
    next.messages.push(Message::user(format!(
        "{}{}",
        note(exchanges.len() - kept.len()),
        kept.join("\n\n")
    )));
    Some(next)
}

fn strip_recent(message: &MessageInfo) -> MessageInfo {
    match message {
        MessageInfo::Compaction(oc_schema::session_message::Compaction::Completed {
            id,
            metadata,
            time,
            reason,
            model,
            provider_state,
            summary,
            ..
        }) => MessageInfo::Compaction(oc_schema::session_message::Compaction::Completed {
            id: id.clone(),
            metadata: metadata.clone(),
            time: *time,
            reason: *reason,
            model: model.clone(),
            provider_state: provider_state.clone(),
            summary: summary.clone(),
            recent: String::new(),
            provider_context: None,
            cost: None,
            tokens: None,
        }),
        other => other.clone(),
    }
}

/// `flattenMessage` (compaction.ts:823-843): one request message as a
/// flattened transcript line.
fn flatten_message(message: &Message) -> String {
    use oc_ai::schema::messages::ContentPart as Part;
    if message.role == MessageRole::System {
        return String::new();
    }
    message
        .content
        .iter()
        .flat_map(|part| -> Vec<String> {
            match part {
                Part::Text { text, .. } => {
                    if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![format!(
                            "[{}]: {text}",
                            if message.role == MessageRole::User { "User" } else { "Assistant" }
                        )]
                    }
                }
                Part::Media { media, .. } => vec![format!("[{} omitted]", media.media_type)],
                Part::ToolCall { name, input, .. } => vec![format!(
                    "[Assistant tool call]: {name}({})",
                    serde_json::to_string(input).unwrap_or_default()
                )],
                Part::ToolResult { result, .. } => {
                    let output = match result {
                        oc_ai::schema::messages::ToolResultValue::Content { value } => value
                            .0
                            .iter()
                            .map(|item| match item.get("type").and_then(Value::as_str) {
                                Some("text") => item
                                    .get("text")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                Some("file") => format!(
                                    "[Attached {}{}]",
                                    item.get("mime").and_then(Value::as_str).unwrap_or_default(),
                                    item.get("name")
                                        .and_then(Value::as_str)
                                        .map(|name| format!(": {name}"))
                                        .unwrap_or_default()
                                ),
                                _ => String::new(),
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                        other => stringify_value(&match other {
                            oc_ai::schema::messages::ToolResultValue::Json { value }
                            | oc_ai::schema::messages::ToolResultValue::Text { value }
                            | oc_ai::schema::messages::ToolResultValue::Error { value } => {
                                value.clone()
                            }
                            oc_ai::schema::messages::ToolResultValue::Content { .. } => Value::Null,
                        }),
                    };
                    let label = if result.kind() == "error" { "Tool error" } else { "Tool result" };
                    vec![format!("[{label}]: {}", truncate_tool_output(&output))]
                }
                _ => Vec::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `recentUserMessages` (compaction.ts:854-866): the newest whole, real
/// user messages within `keep` tokens — no synthetic guidance, no half of
/// an attachment.
#[must_use]
pub fn recent_user_messages(
    messages: &[MessageInfo],
    model: &ResolvedModel,
    keep: i64,
) -> Vec<Message> {
    let users: Vec<MessageInfo> = messages
        .iter()
        .filter(|message| matches!(message, MessageInfo::User { .. }))
        .map(|message| {
            let MessageInfo::User { id, metadata, time, text, files, agents, .. } = message else {
                return message.clone();
            };
            MessageInfo::User {
                id: id.clone(),
                metadata: metadata.clone(),
                time: *time,
                text: text.clone(),
                files: files.clone(),
                agents: agents.clone(),
                skills: None,
            }
        })
        .collect();
    let lowered = crate::session::runner::to_llm_message::to_llm_messages(
        &users,
        &model.reference,
        model.provider_metadata_key(),
    );
    let sendable = bound_images(unsupported_parts(lowered, &model.capabilities));
    let sizes: Vec<i64> = sendable.iter().map(estimate_message).collect();
    let from = oldest_to_drop(&sizes, keep);
    sendable[from.min(sendable.len())..].to_vec()
}
