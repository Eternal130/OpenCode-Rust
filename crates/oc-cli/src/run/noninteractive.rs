//! Port of `cli/src/run/noninteractive.ts` — the SSE-driven prompt loop:
//! event consumption, v2's exact rendering (step markers, tool lines, text
//! streaming via `run_ui`), JSON event emission, permission auto-reply
//! (TTY prompt via dialoguer with once/always/deny; non-TTY auto-rejects
//! with model-visible feedback), form cancellation, message reconciliation,
//! Ctrl-C interrupt propagation, and exit-code semantics.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use oc_client::types::{
    ModelRef, PermissionReply, PromptInputFileAttachment, SessionInboxDelivery,
    SessionPermissionReplyBody, SessionPromptBody,
};
use oc_client::Client;
use serde_json::{json, Value};

use crate::run::tool_render;
use crate::util;
use crate::{run_ui, run_ui::style};

const GLOBAL_FORM_SESSION_ID: &str = "global";

const QUESTION_CANCELLED_FEEDBACK: &str = "This non-interactive run cannot ask the user questions, so the question was cancelled. Continue without an answer; make reasonable assumptions and state them.";

const PERMISSION_REJECTED_FEEDBACK: &str = "This non-interactive run cannot ask the user for permission, so the request was rejected. Continue without this action.";

/// `--format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Default,
    Json,
}

/// A prepared `-f/--file` attachment (run.ts `prepareFile`).
#[derive(Debug, Clone)]
pub enum PreparedFile {
    Text { filename: String, content: String },
    Attachment { filename: String, uri: String },
}

/// The prompt-loop input (runNonInteractivePrompt's `Input`).
pub struct PromptInput {
    pub client: Client,
    pub session_id: String,
    pub directory: String,
    pub message: String,
    pub files: Vec<PreparedFile>,
    pub agent: Option<String>,
    pub model: Option<ModelRef>,
    pub thinking: bool,
    pub format: Format,
    pub auto: bool,
}

#[derive(Debug, Clone, Default)]
struct StartedPart {
    id: String,
    timestamp: u64,
}

#[derive(Debug, Clone, Default)]
struct ToolState {
    id: String,
    timestamp: u64,
    tool: String,
    input: Value,
    raw: Option<String>,
    provider_call: Option<Value>,
    metadata: Value,
    content: Vec<Value>,
}

#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
struct RenderState {
    starts: HashMap<String, StartedPart>,
    tools: HashMap<String, ToolState>,
    rendered_text: HashMap<String, String>,
    rendered_reasoning: HashMap<String, String>,
    rendered_tools: HashSet<String>,
    owned: HashMap<String, bool>,
    submitted: bool,
    promoted: bool,
    emitted_error: bool,
    permission_rejected: bool,
    form_cancelled: bool,
    interrupted: bool,
    pre_promotion_error: Option<Value>,
    finalizing: bool,
    exit_code: i32,
}

struct Loop {
    client: Client,
    session_id: String,
    message_id: String,
    directory: String,
    thinking: bool,
    format: Format,
    auto: bool,
    stdout_tty: bool,
    state: Arc<Mutex<RenderState>>,
}

fn part_id(event_id: &str) -> String {
    format!("prt_{}", event_id.strip_prefix("evt_").unwrap_or(event_id))
}

fn content_key(message_id: &str, ordinal: i64) -> String {
    format!("{message_id}\0{ordinal}")
}

fn tool_key(message_id: &str, id: &str) -> String {
    format!("{message_id}\0{id}")
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

fn text_field(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn i64_field(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

impl Loop {
    fn lock(&self) -> std::sync::MutexGuard<'_, RenderState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `emit`: JSON-lines output (the only thing `--format json` prints).
    fn emit(&self, kind: &str, timestamp: u64, data: &Value) -> bool {
        if self.format != Format::Json {
            return false;
        }
        let mut line = json!({
            "type": kind,
            "timestamp": timestamp,
            "sessionID": self.session_id,
        });
        if let (Some(object), Some(extra)) = (line.as_object_mut(), data.as_object()) {
            for (key, value) in extra {
                object.insert(key.clone(), value.clone());
            }
        }
        println!("{line}");
        true
    }

    /// `writeText`: assistant text → stdout.
    fn write_text(&self, part: &Value, timestamp: u64) {
        if self.emit("text", timestamp, &json!({ "part": part })) {
            return;
        }
        let text = text_field(part, "text").trim().to_owned();
        if text.is_empty() {
            return;
        }
        if !self.stdout_tty {
            println!("{text}");
            return;
        }
        run_ui::empty();
        run_ui::println(&[&text]);
        run_ui::empty();
    }

    /// `writeReasoning`: thinking blocks → stderr, dim + italic.
    fn write_reasoning(&self, part: &Value, timestamp: u64) {
        if self.emit("reasoning", timestamp, &json!({ "part": part })) {
            return;
        }
        let text = text_field(part, "text").trim().to_owned();
        if text.is_empty() {
            return;
        }
        let line = format!("Thinking: {text}");
        if !self.stdout_tty {
            run_ui::println(&[&line]);
            return;
        }
        let dim = style::TEXT_DIM;
        let normal = style::TEXT_NORMAL;
        run_ui::println(&[&format!("{dim}\x1b[3m{line}\x1b[0m{normal}")]);
        run_ui::empty();
    }

    /// `renderTool` (run.ts): icon + title (+ dim description); block mode
    /// prints the body between blank lines.
    #[allow(clippy::unused_self)]
    fn render_tool(&self, info: &tool_render::ToolInline) {
        if info.block {
            run_ui::empty();
            run_ui::println(&[
                &format!("{}{}", style::TEXT_NORMAL, info.icon),
                &format!("{}{}", style::TEXT_NORMAL, info.title),
            ]);
            if let Some(body) = &info.body {
                if !body.trim().is_empty() {
                    run_ui::println(&[body]);
                }
            }
            run_ui::empty();
            return;
        }
        let dim = style::TEXT_DIM;
        let normal = style::TEXT_NORMAL;
        let parts: Vec<String> = match &info.description {
            Some(description) => vec![
                format!("{normal}{}", info.icon),
                format!("{normal}{}", info.title),
                format!("{dim}{description}{normal}"),
            ],
            None => vec![format!("{normal}{}", info.icon), format!("{normal}{}", info.title)],
        };
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        run_ui::println(&refs);
    }

    #[allow(clippy::unused_self)]
    fn render_tool_error(&self, info: &tool_render::ToolInline) {
        run_ui::println(&[
            &format!("{}✗", style::TEXT_NORMAL),
            &format!("{}{} failed", style::TEXT_NORMAL, info.title),
        ]);
    }

    fn inline_info(&self, tool: &ToolRenderPart) -> tool_render::ToolInline {
        let input = tool.input.as_object().cloned().unwrap_or_default();
        let metadata = tool.metadata.as_object().cloned().unwrap_or_default();
        tool_render::tool_inline_info(
            &tool.tool,
            &input,
            &metadata,
            &tool.status,
            &tool.output,
            &self.directory,
        )
    }

    /// `replyPermission`: outside a TTY nobody can approve — reject with
    /// model-visible feedback so the run continues. `--auto` approves once.
    /// On a TTY (wave-6a spec) ask once/always/deny via dialoguer.
    async fn reply_permission(&self, request: &Value) {
        let request_id = text_field(request, "id");
        let session_id = text_field(request, "sessionID");
        let action = text_field(request, "action");
        let resources: Vec<String> = request
            .get("resources")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default();

        let decision = if self.auto {
            "once".to_owned()
        } else if permission_tty_prompt_enabled() && util::stdout_is_tty() && util::stdin_is_tty() {
            if let Some(choice) = permission_prompt(&action, &resources) {
                choice
            } else {
                // Ctrl-C at the prompt behaves like a user interrupt.
                self.lock().interrupted = true;
                let _ = self
                    .client
                    .session()
                    .interrupt(
                        &self.session_id,
                        &oc_client::types::SessionInterruptQuery { resume: None },
                    )
                    .await;
                "reject".to_owned()
            }
        } else {
            run_ui::println(&[
                &format!("{}!", style::TEXT_WARNING_BOLD),
                &format!(
                    "{}permission requested: {action} ({}); auto-rejecting",
                    style::TEXT_NORMAL,
                    resources.join(", ")
                ),
            ]);
            "reject".to_owned()
        };
        let body = SessionPermissionReplyBody {
            decision: match decision.as_str() {
                "once" => PermissionReply::Once,
                "always" => PermissionReply::Always,
                _ => PermissionReply::Reject,
            },
            message: (decision == "reject").then(|| PERMISSION_REJECTED_FEEDBACK.to_owned()),
        };
        let _ = self.client.session().permission_reply(&session_id, &request_id, &body).await;
    }

    /// `cancelForm`: question forms are cancelled with model-visible
    /// feedback; anything else fails the run.
    async fn cancel_form(&self, form: &Value) {
        let form_id = text_field(form, "id");
        let session_id = text_field(form, "sessionID");
        let kind = form
            .get("metadata")
            .and_then(|metadata| metadata.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let continuing = kind == "question";
        let query = oc_client::types::SessionFormCancelQuery {
            message: continuing.then(|| QUESTION_CANCELLED_FEEDBACK.to_owned()),
        };
        let mut client = self.client.clone();
        if session_id == GLOBAL_FORM_SESSION_ID {
            let directory = urlencoding(&self.directory);
            if let Ok(with_header) =
                client.clone().with_default_header("x-opencode-directory", &directory)
            {
                client = with_header;
            }
        }
        let _ = client.session().form().cancel(&session_id, &form_id, &query).await;
        if continuing {
            return;
        }
        self.lock().form_cancelled = true;
    }

    /// `ownsSession`: this run's session plus its child (subagent) sessions.
    async fn owns_session(&self, session_id: &str) -> bool {
        if let Some(known) = self.lock().owned.get(session_id) {
            return *known;
        }
        let owned = if session_id == GLOBAL_FORM_SESSION_ID {
            false
        } else {
            match self.client.session().get(session_id).await {
                Ok(session) => match session.parent_id {
                    Some(parent) => Box::pin(self.owns_session(&parent)).await,
                    None => false,
                },
                Err(_) => false,
            }
        };
        self.lock().owned.insert(session_id.to_owned(), owned);
        owned
    }

    fn record_exit(&self, code: i32) {
        let mut state = self.lock();
        if state.exit_code == 0 {
            state.exit_code = code;
        }
    }
}

fn permission_prompt(action: &str, resources: &[String]) -> Option<String> {
    use dialoguer::Select;
    let prompt = if resources.is_empty() {
        format!("Permission: {action}")
    } else {
        format!("Permission: {action} ({})", resources.join(", "))
    };
    let selection = Select::new()
        .with_prompt(prompt)
        .items(&["once", "always", "deny"])
        .default(0)
        .interact_opt()
        .ok()
        .flatten();
    match selection {
        Some(0) => Some("once".to_owned()),
        Some(1) => Some("always".to_owned()),
        Some(_) => Some("reject".to_owned()),
        None => None,
    }
}

/// Kill-switch for the interactive permission prompt: `CI` set (automation
/// harnesses, expect scripts) or `OPENCODE_NO_TTY_PROMPT=1` force the
/// non-TTY auto-reject path even on a real TTY, where a dialoguer prompt
/// would hang forever with nobody to answer it (oracle gate-7 decision).
fn permission_tty_prompt_enabled() -> bool {
    if std::env::var_os("CI").is_some() {
        return false;
    }
    !matches!(std::env::var("OPENCODE_NO_TTY_PROMPT"), Ok(value) if value == "1")
}

fn urlencoding(input: &str) -> String {
    // encodeURIComponent equivalence for path characters.
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            _ => {
                out.push('%');
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 0xf) as usize] as char);
            }
        }
    }
    out
}

#[derive(Clone)]
struct ToolRenderPart {
    tool: String,
    input: Value,
    metadata: Value,
    status: String,
    output: String,
}

/// The main entry: returns the process exit code.
#[allow(clippy::too_many_lines)]
pub async fn run_prompt(input: PromptInput) -> i32 {
    let PromptInput {
        client,
        session_id,
        directory,
        message,
        files,
        agent,
        model,
        thinking,
        format,
        auto,
    } = input;

    // Agent/model selection happens before the prompt is submitted.
    if let Some(agent) = &agent {
        let _ = client
            .session()
            .switch_agent(
                &session_id,
                &oc_client::types::SessionSwitchAgentBody { agent: agent.clone() },
            )
            .await;
    }
    if let Some(model) = &model {
        let _ = client
            .session()
            .switch_model(
                &session_id,
                &oc_client::types::SessionSwitchModelBody {
                    model: ModelRef {
                        provider_id: model.provider_id.clone(),
                        id: model.id.clone(),
                        variant: model.variant.clone(),
                    },
                },
            )
            .await;
    }

    let mut stream = match client.event().subscribe().await {
        Ok(stream) => stream,
        Err(error) => {
            report_error(format, &session_id, &util::error_message(&error.to_string()));
            return 1;
        }
    };
    // The first frame is `server.connected` — prompt admission depends on it.
    if !matches!(stream.next_json().await, Ok(Some(_))) {
        report_error(format, &session_id, "Event stream disconnected before prompt admission");
        return 1;
    }

    let message_id = format!("msg_{}", oc_schema::identifier::ascending());
    let state = Arc::new(Mutex::new(RenderState {
        owned: HashMap::from([(session_id.clone(), true)]),
        submitted: true,
        ..Default::default()
    }));
    let stdout_tty = util::stdout_is_tty();
    let loop_ctx = Loop {
        client: client.clone(),
        session_id: session_id.clone(),
        message_id: message_id.clone(),
        directory: directory.clone(),
        thinking,
        format,
        auto,
        stdout_tty,
        state: Arc::clone(&state),
    };

    // Ctrl-C: first press interrupts the session, second exits immediately.
    let interrupt_state = Arc::clone(&state);
    let interrupt_client = client.clone();
    let interrupt_session = session_id.clone();
    let ctrl_c = tokio::spawn(async move {
        loop {
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
            let already = {
                let mut state =
                    interrupt_state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if state.interrupted {
                    std::process::exit(130);
                }
                state.interrupted = true;
                state.exit_code = 130;
                true
            };
            let _ = already;
            let _ = interrupt_client
                .session()
                .interrupt(
                    &interrupt_session,
                    &oc_client::types::SessionInterruptQuery { resume: None },
                )
                .await;
        }
    });

    // Consume events while the prompt is in flight.
    let consumer = {
        let ctx = Loop {
            client: client.clone(),
            session_id: session_id.clone(),
            message_id: message_id.clone(),
            directory: directory.clone(),
            thinking,
            format,
            auto,
            stdout_tty,
            state: Arc::clone(&state),
        };
        tokio::spawn(consume(ctx, stream))
    };

    let mut body_text = message.clone();
    let mut attachments: Vec<PromptInputFileAttachment> = Vec::new();
    for file in &files {
        match file {
            PreparedFile::Text { filename, content } => {
                body_text.push_str("\n\n<file name=\"");
                body_text.push_str(filename);
                body_text.push_str("\">\n");
                body_text.push_str(content);
                body_text.push_str("\n</file>");
            }
            PreparedFile::Attachment { uri, filename, .. } => {
                attachments.push(PromptInputFileAttachment {
                    uri: uri.clone(),
                    name: Some(filename.clone()),
                    ..Default::default()
                });
            }
        }
    }
    let prompt_body = SessionPromptBody {
        id: Some(message_id.clone()),
        text: body_text,
        files: (!attachments.is_empty()).then_some(attachments),
        delivery: Some(SessionInboxDelivery::Steer),
        ..Default::default()
    };
    let prompt_result = client.session().prompt(&session_id, &prompt_body).await;

    // Leftover prompts that raced the subscription: answer them now.
    if let Ok(permissions) = client.session().permission_list(&session_id).await {
        for request in permissions {
            let value = serde_json::to_value(&request).expect("permission serializes");
            loop_ctx.reply_permission(&value).await;
        }
    }
    if let Ok(forms) = client.session().form().list(&session_id).await {
        for form in forms {
            let value = serde_json::to_value(&form).expect("form serializes");
            loop_ctx.cancel_form(&value).await;
        }
    }

    if let Err(error) = prompt_result {
        ctrl_c.abort();
        let interrupted = loop_ctx.lock().interrupted;
        if !interrupted {
            report_error(format, &session_id, &util::error_message(&error.to_string()));
            return 1;
        }
        return 130;
    }

    // Race the session wait against the consumer's own termination.
    let mut consumer = consumer;
    let session_api = client.session();
    let wait = session_api.wait(&session_id);
    tokio::pin!(wait);
    // A JoinHandle may only be polled to completion ONCE (tokio panics on
    // a double-poll), so every later drain/await is gated on whether an
    // earlier poll already finished it.
    let mut consumer_done = false;
    tokio::select! {
        result = &mut wait => {
            if let Err(error) = result {
                // `wait` is an output-completeness optimization; the
                // reconciliation below remains the source of truth.
                let _ = error;
            }
        }
        _ = &mut consumer => {
            // The consumer's terminal event beat `session.wait` — the
            // handle is complete; later polls must be skipped.
            consumer_done = true;
        }
    }
    // ALWAYS drain the consumer before reconciling. `wait` can resolve the
    // instant execution ends while the consumer task has not been polled
    // yet — abandoning it there loses the step markers (reconcile only
    // re-renders text/tools). v2 gets this ordering implicitly from the
    // single-threaded event loop; a bounded await is our equivalent. The
    // consumer always terminates (execution.succeeded/failed/interrupted),
    // so the timeout only guards pathological stalls.
    if !consumer_done
        && tokio::time::timeout(std::time::Duration::from_secs(30), &mut consumer).await.is_ok()
    {
        consumer_done = true;
    }
    loop_ctx.lock().finalizing = true;

    let (found, responded) = reconcile(&loop_ctx, &client).await;
    let await_consumer = {
        let state = loop_ctx.lock();
        !responded
            && !state.interrupted
            && !state.permission_rejected
            && !state.form_cancelled
            && !state.emitted_error
            && state.pre_promotion_error.is_none()
    };
    if await_consumer && !consumer_done {
        let _ = consumer.await;
    }
    ctrl_c.abort();
    let state = loop_ctx.lock();
    if !found
        && !state.interrupted
        && !state.permission_rejected
        && !state.form_cancelled
        && !state.emitted_error
    {
        let error = state
            .pre_promotion_error
            .clone()
            .unwrap_or_else(|| json!({ "type": "unknown", "message": "Prompt was not promoted" }));
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Prompt was not promoted")
            .to_owned();
        drop(state);
        loop_ctx.lock().emitted_error = true;
        if !loop_ctx.emit("error", util::now_ms(), &json!({ "error": error })) {
            run_ui::error(&message);
        }
        return 1;
    }
    if state.exit_code != 0 {
        return state.exit_code;
    }
    0
}

fn report_error(format: Format, session_id: &str, message: &str) {
    if format == Format::Json {
        println!(
            "{}",
            json!({
                "type": "error",
                "timestamp": util::now_ms(),
                "sessionID": session_id,
                "error": { "type": "unknown", "message": message },
            })
        );
        return;
    }
    run_ui::error(message);
}

/// The consume loop: renders events until a terminal execution event.
#[allow(clippy::too_many_lines)]
async fn consume(ctx: Loop, mut stream: oc_client::SseStream) {
    loop {
        let Ok(Some(event)) = stream.next_json().await else {
            let emitted = ctx.lock().emitted_error;
            if !emitted {
                report_error(
                    ctx.format,
                    &ctx.session_id,
                    "Event stream disconnected during prompt execution",
                );
                ctx.record_exit(1);
            }
            return;
        };
        let event_type = text_field(&event, "type");
        let data = field(&event, "data").clone();
        let time = event.get("created").and_then(Value::as_u64).unwrap_or_else(util::now_ms);
        let event_id = text_field(&event, "id");

        // Cross-session prompts first (any session this run owns).
        if event_type == "permission.asked" {
            let session_id = text_field(&data, "sessionID");
            if ctx.lock().submitted && ctx.owns_session(&session_id).await {
                ctx.reply_permission(&data).await;
            }
            continue;
        }
        if event_type == "form.created" {
            let form = field(&data, "form");
            let session_id = text_field(form, "sessionID");
            if ctx.lock().submitted && ctx.owns_session(&session_id).await {
                ctx.cancel_form(form).await;
            }
            continue;
        }

        if text_field(&data, "sessionID") != ctx.session_id {
            continue;
        }

        if event_type == "session.inbox.delivered" {
            if text_field(&data, "inboxID") == ctx.message_id {
                let mut state = ctx.lock();
                state.promoted = true;
                state.pre_promotion_error = None;
            }
            continue;
        }
        if event_type == "session.execution.interrupted" {
            let reason = text_field(&data, "reason");
            let terminal = {
                let state = ctx.lock();
                state.interrupted || state.permission_rejected || state.form_cancelled
            };
            if reason == "user" && terminal {
                return;
            }
        }

        let promoted = ctx.lock().promoted;
        if !promoted {
            if event_type == "session.execution.failed" {
                let finalizing = {
                    let mut state = ctx.lock();
                    state.pre_promotion_error = data.get("error").cloned();
                    state.finalizing
                };
                if finalizing {
                    return;
                }
                continue;
            }
            if (event_type == "session.execution.succeeded"
                || event_type == "session.execution.interrupted")
                && ctx.lock().finalizing
            {
                return;
            }
            continue;
        }

        if ctx.lock().finalizing && !event_type.starts_with("session.execution.") {
            continue;
        }

        match event_type.as_str() {
            "session.step.started" => {
                let part = json!({
                    "id": part_id(&event_id),
                    "sessionID": ctx.session_id,
                    "messageID": text_field(&data, "assistantMessageID"),
                    "type": "step-start",
                    "snapshot": field(&data, "snapshot"),
                });
                if !ctx.emit("step_start", time, &json!({ "part": part })) {
                    let label = format!(
                        "> {} · {}",
                        text_field(&data, "agent"),
                        field(&data, "model").get("id").and_then(Value::as_str).unwrap_or_default()
                    );
                    run_ui::empty();
                    run_ui::println(&[&label]);
                    run_ui::empty();
                }
            }
            "session.text.started" => {
                let key = format!(
                    "text\0{}",
                    content_key(
                        &text_field(&data, "assistantMessageID"),
                        i64_field(&data, "ordinal").unwrap_or(0)
                    )
                );
                ctx.lock()
                    .starts
                    .insert(key, StartedPart { id: part_id(&event_id), timestamp: time });
            }
            "session.text.ended" => {
                let message_id = text_field(&data, "assistantMessageID");
                let ordinal = i64_field(&data, "ordinal").unwrap_or(0);
                let key = content_key(&message_id, ordinal);
                let started = ctx.lock().starts.remove(&format!("text\0{key}"));
                let text = text_field(&data, "text");
                let part = json!({
                    "id": started.as_ref().map_or_else(|| part_id(&event_id), |s| s.id.clone()),
                    "sessionID": ctx.session_id,
                    "messageID": message_id,
                    "type": "text",
                    "text": text,
                    "time": {
                        "start": started.as_ref().map_or(time, |s| s.timestamp),
                        "end": time,
                    },
                });
                ctx.lock().rendered_text.insert(key, text);
                ctx.write_text(&part, time);
            }
            "session.reasoning.started" => {
                let key = format!(
                    "reasoning\0{}",
                    content_key(
                        &text_field(&data, "assistantMessageID"),
                        i64_field(&data, "ordinal").unwrap_or(0)
                    )
                );
                ctx.lock()
                    .starts
                    .insert(key, StartedPart { id: part_id(&event_id), timestamp: time });
            }
            "session.reasoning.ended" if ctx.thinking => {
                let message_id = text_field(&data, "assistantMessageID");
                let ordinal = i64_field(&data, "ordinal").unwrap_or(0);
                let key = content_key(&message_id, ordinal);
                let started = ctx.lock().starts.remove(&format!("reasoning\0{key}"));
                let text = text_field(&data, "text");
                let part = json!({
                    "id": started.as_ref().map_or_else(|| part_id(&event_id), |s| s.id.clone()),
                    "sessionID": ctx.session_id,
                    "messageID": message_id,
                    "type": "reasoning",
                    "text": text,
                    "metadata": field(&data, "state"),
                    "time": {
                        "start": started.as_ref().map_or(time, |s| s.timestamp),
                        "end": time,
                    },
                });
                ctx.lock().rendered_reasoning.insert(key, text);
                ctx.write_reasoning(&part, time);
            }
            "session.tool.input.started" => {
                let key =
                    tool_key(&text_field(&data, "assistantMessageID"), &text_field(&data, "id"));
                ctx.lock().tools.insert(
                    key,
                    ToolState {
                        id: part_id(&event_id),
                        timestamp: time,
                        tool: text_field(&data, "name"),
                        ..Default::default()
                    },
                );
            }
            "session.tool.input.ended" => {
                let key =
                    tool_key(&text_field(&data, "assistantMessageID"), &text_field(&data, "id"));
                if let Some(current) = ctx.lock().tools.get_mut(&key) {
                    current.raw = Some(text_field(&data, "text"));
                }
            }
            "session.tool.input.delta" => {
                let key =
                    tool_key(&text_field(&data, "assistantMessageID"), &text_field(&data, "id"));
                if let Some(current) = ctx.lock().tools.get_mut(&key) {
                    let raw = current.raw.take().unwrap_or_default();
                    current.raw = Some(format!("{raw}{}", text_field(&data, "delta")));
                }
            }
            "session.tool.called" => {
                let key =
                    tool_key(&text_field(&data, "assistantMessageID"), &text_field(&data, "id"));
                let current = ctx.lock().tools.remove(&key);
                let provider_call = json!({
                    "executed": field(&data, "executed"),
                    "state": field(&data, "state"),
                });
                let tool = ToolState {
                    id: current.as_ref().map_or_else(|| part_id(&event_id), |c| c.id.clone()),
                    timestamp: current.as_ref().map_or(time, |c| c.timestamp),
                    tool: current.as_ref().map_or_else(|| "tool".to_owned(), |c| c.tool.clone()),
                    input: field(&data, "input").clone(),
                    raw: current.as_ref().and_then(|c| c.raw.clone()),
                    provider_call: Some(provider_call),
                    metadata: json!({}),
                    content: Vec::new(),
                };
                ctx.lock().tools.insert(key, tool);
            }
            "session.tool.progress" => {
                let key =
                    tool_key(&text_field(&data, "assistantMessageID"), &text_field(&data, "id"));
                if let Some(current) = ctx.lock().tools.get_mut(&key) {
                    current.metadata = field(&data, "metadata").clone();
                }
            }
            "session.tool.success" => {
                let key =
                    tool_key(&text_field(&data, "assistantMessageID"), &text_field(&data, "id"));
                let current = ctx
                    .lock()
                    .tools
                    .remove(&key)
                    .unwrap_or_else(|| fallback_tool(&event_id, time, &data));
                let content = field(&data, "content").as_array().cloned().unwrap_or_default();
                let metadata = field(&data, "metadata");
                let output = tool_render::tool_output_text(&current.tool, Some(&content));
                let part = json!({
                    "partID": current.id,
                    "sessionID": ctx.session_id,
                    "messageID": text_field(&data, "assistantMessageID"),
                    "type": "tool",
                    "id": text_field(&data, "id"),
                    "tool": current.tool,
                    "state": {
                        "status": "completed",
                        "input": current.input,
                        "output": output,
                        "title": current.tool,
                        "metadata": {
                            "metadata": metadata,
                            "content": content,
                            "providerCall": current.provider_call,
                            "providerResult": {
                                "executed": field(&data, "executed"),
                                "state": field(&data, "resultState"),
                            },
                            "rawInput": current.raw,
                        },
                        "time": { "start": current.timestamp, "end": time },
                    },
                });
                ctx.lock().rendered_tools.insert(key);
                if !ctx.emit("tool_use", time, &json!({ "part": part })) {
                    let render = ToolRenderPart {
                        tool: current.tool,
                        input: current.input,
                        metadata: metadata.clone(),
                        status: "completed".to_owned(),
                        output,
                    };
                    ctx.render_tool(&ctx.inline_info(&render));
                }
            }
            "session.tool.failed" => {
                let key =
                    tool_key(&text_field(&data, "assistantMessageID"), &text_field(&data, "id"));
                let current = ctx
                    .lock()
                    .tools
                    .remove(&key)
                    .unwrap_or_else(|| fallback_tool(&event_id, time, &data));
                let error = field(&data, "error")
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let metadata = if data.get("metadata").is_none_or(Value::is_null) {
                    current.metadata.clone()
                } else {
                    field(&data, "metadata").clone()
                };
                let content = data
                    .get("content")
                    .and_then(Value::as_array)
                    .cloned()
                    .filter(|content| !content.is_empty())
                    .unwrap_or_else(|| current.content.clone());
                ctx.lock().rendered_tools.insert(key);
                let output = tool_render::tool_output_text(&current.tool, Some(&content));
                // v2's failed-tool part: machine consumers get the tool
                // identity, input, error text, and the provider call
                // envelope (noninteractive.ts `session.tool.failed`).
                let part = json!({
                    "partID": current.id,
                    "sessionID": ctx.session_id,
                    "messageID": text_field(&data, "assistantMessageID"),
                    "type": "tool",
                    "id": text_field(&data, "id"),
                    "tool": current.tool,
                    "state": {
                        "status": "error",
                        "input": current.input,
                        "error": error,
                        "metadata": {
                            "providerCall": current.provider_call,
                            "providerResult": {
                                "executed": field(&data, "executed"),
                                "state": field(&data, "resultState"),
                            },
                            "rawInput": current.raw,
                        },
                        "time": { "start": current.timestamp, "end": time },
                    },
                });
                if !ctx.emit("tool_use", time, &json!({ "part": part })) {
                    let render = ToolRenderPart {
                        tool: current.tool,
                        input: current.input,
                        metadata,
                        status: "error".to_owned(),
                        output: output.clone(),
                    };
                    if !output.trim().is_empty() {
                        let completed =
                            ToolRenderPart { status: "completed".to_owned(), ..render.clone() };
                        ctx.render_tool(&ctx.inline_info(&completed));
                    }
                    ctx.render_tool_error(&ctx.inline_info(&render));
                    run_ui::error(&error);
                }
            }
            "session.step.ended" => {
                let part = json!({
                    "id": part_id(&event_id),
                    "sessionID": ctx.session_id,
                    "messageID": text_field(&data, "assistantMessageID"),
                    "type": "step-finish",
                    "reason": field(&data, "finish"),
                    "snapshot": field(&data, "snapshot"),
                    "cost": field(&data, "cost"),
                    "tokens": field(&data, "tokens"),
                });
                ctx.emit("step_finish", time, &json!({ "part": part }));
            }
            "session.step.failed" => {
                let skip = {
                    let state = ctx.lock();
                    state.interrupted || state.permission_rejected || state.form_cancelled
                };
                if skip {
                    continue;
                }
                let error = field(&data, "error");
                let message = error.get("message").and_then(Value::as_str).unwrap_or_default();
                {
                    let mut state = ctx.lock();
                    state.emitted_error = true;
                    state.exit_code = 1;
                }
                if !ctx.emit("error", time, &json!({ "error": error })) {
                    run_ui::error(message);
                }
            }
            "session.execution.failed" => {
                let already = {
                    let state = ctx.lock();
                    state.emitted_error || state.form_cancelled
                };
                if !already {
                    let error = field(&data, "error");
                    let message =
                        error.get("message").and_then(Value::as_str).unwrap_or_default().to_owned();
                    ctx.lock().emitted_error = true;
                    ctx.record_exit(1);
                    if !ctx.emit("error", time, &json!({ "error": error })) {
                        run_ui::error(&message);
                    }
                }
                return;
            }
            "session.execution.interrupted" => {
                let reason = text_field(&data, "reason");
                let skip_rest = {
                    let mut state = ctx.lock();
                    if reason == "user" && state.interrupted && state.exit_code == 0 {
                        state.exit_code = 130;
                    }
                    state.emitted_error || state.permission_rejected || state.form_cancelled
                };
                if reason != "user" && !skip_rest {
                    let message = format!("Session interrupted: {reason}");
                    ctx.lock().emitted_error = true;
                    ctx.record_exit(1);
                    if !ctx.emit(
                        "error",
                        time,
                        &json!({ "error": { "type": "aborted", "message": message } }),
                    ) {
                        run_ui::error(&message);
                    }
                }
                return;
            }
            "session.execution.succeeded" => return,
            _ => {}
        }
    }
}

fn fallback_tool(event_id: &str, time: u64, _data: &Value) -> ToolState {
    ToolState {
        id: part_id(event_id),
        timestamp: time,
        tool: "tool".to_owned(),
        input: json!({}),
        raw: None,
        provider_call: None,
        metadata: json!({}),
        content: Vec::new(),
    }
}

/// `reconcile`: page through the projected messages looking for ours; render
/// anything the event stream missed. Returns `(found, responded)`. When our
/// message is NOT in the projection, v2 discards the collected messages
/// (`found: false → messages: []`) — nothing renders and `responded` is
/// false, so a failed run on a resumed session never replays older turns.
#[allow(clippy::too_many_lines)]
async fn reconcile(ctx: &Loop, client: &Client) -> (bool, bool) {
    let mut messages: Vec<Value> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut found = false;
    loop {
        let query = oc_client::types::SessionMessageListQuery {
            cursor: cursor.clone(),
            limit: Some("200".to_owned()),
            order: Some(oc_client::types::SessionMessageListQueryOrder::Desc),
            type_: None,
        };
        let Ok(page) = client.message().list(&ctx.session_id, &query).await else {
            return (false, false);
        };
        for message in &page.data {
            let value = serde_json::to_value(message).expect("message serializes");
            if value.get("id").and_then(Value::as_str) == Some(ctx.message_id.as_str()) {
                found = true;
                break;
            }
            messages.push(value);
        }
        if found || page.cursor.next.is_none() {
            break;
        }
        cursor.clone_from(&page.cursor.next);
    }
    if !found {
        return (false, false);
    }
    messages.reverse();
    let mut responded = false;

    for message in &messages {
        if message.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        responded = true;
        let times = field(message, "time");
        let timestamp = times
            .get("completed")
            .or_else(|| times.get("created"))
            .and_then(Value::as_u64)
            .unwrap_or_else(util::now_ms);
        let message_id = text_field(message, "id");
        let mut text_ordinal = 0i64;
        let mut reasoning_ordinal = 0i64;
        let empty: Vec<Value> = Vec::new();
        let content_items: &[Value] = message
            .get("content")
            .and_then(Value::as_array)
            .map_or(empty.as_slice(), std::vec::Vec::as_slice);
        for item in content_items {
            let item_type = text_field(item, "type");
            if item_type == "text" {
                let ordinal = text_ordinal;
                text_ordinal += 1;
                let key = content_key(&message_id, ordinal);
                let item_text = text_field(item, "text");
                let rendered = ctx.lock().rendered_text.get(&key).cloned().unwrap_or_default();
                if rendered == item_text || !item_text.starts_with(&rendered) {
                    continue;
                }
                let remainder = item_text[rendered.len()..].to_owned();
                let part = json!({
                    "id": format!("prt_{}_text-{ordinal}", message_id.trim_start_matches("msg_")),
                    "sessionID": ctx.session_id,
                    "messageID": message_id,
                    "type": "text",
                    "text": remainder,
                    "time": { "start": field(times, "created"), "end": timestamp },
                });
                ctx.lock().rendered_text.insert(key, item_text);
                ctx.write_text(&part, timestamp);
                continue;
            }
            if item_type == "reasoning" {
                let ordinal = reasoning_ordinal;
                reasoning_ordinal += 1;
                if !ctx.thinking {
                    continue;
                }
                let key = content_key(&message_id, ordinal);
                let item_text = text_field(item, "text");
                let rendered = ctx.lock().rendered_reasoning.get(&key).cloned().unwrap_or_default();
                if rendered == item_text || !item_text.starts_with(&rendered) {
                    continue;
                }
                let remainder = item_text[rendered.len()..].to_owned();
                let part = json!({
                    "id": format!("prt_{}_reasoning-{ordinal}", message_id.trim_start_matches("msg_")),
                    "sessionID": ctx.session_id,
                    "messageID": message_id,
                    "type": "reasoning",
                    "text": remainder,
                    "metadata": field(item, "state"),
                    "time": { "start": field(times, "created"), "end": timestamp },
                });
                ctx.lock().rendered_reasoning.insert(key, item_text);
                ctx.write_reasoning(&part, timestamp);
                continue;
            }
            if item_type != "tool" {
                continue;
            }
            let tool_id = text_field(item, "id");
            let key = tool_key(&message_id, &tool_id);
            if ctx.lock().rendered_tools.contains(&key) {
                continue;
            }
            let state = field(item, "state");
            let status = text_field(state, "status");
            if status == "streaming" || status == "running" {
                continue;
            }
            let content =
                state.get("content").and_then(Value::as_array).cloned().unwrap_or_default();
            let output = tool_render::tool_output_text(&text_field(item, "name"), Some(&content));
            let part = json!({
                "partID": format!("prt_{}_tool-{tool_id}", message_id.trim_start_matches("msg_")),
                "sessionID": ctx.session_id,
                "messageID": message_id,
                "type": "tool",
                "id": tool_id,
                "tool": text_field(item, "name"),
                "state": {
                    "status": if status == "completed" { "completed" } else { "error" },
                    "input": field(state, "input"),
                    "output": if status == "completed" { Value::String(output.clone()) } else { Value::Null },
                    "error": if status == "error" {
                        field(state, "error").get("message").cloned().unwrap_or(Value::Null)
                    } else {
                        Value::Null
                    },
                    "metadata": { "metadata": field(state, "metadata"), "content": content },
                    "time": {
                        "start": field(state, "time").get("ran").or_else(|| field(state, "time").get("created")).cloned().unwrap_or(Value::from(timestamp)),
                        "end": field(state, "time").get("completed").cloned().unwrap_or(Value::from(timestamp)),
                    },
                },
            });
            ctx.lock().rendered_tools.insert(key);
            if ctx.emit("tool_use", timestamp, &json!({ "part": part })) {
                continue;
            }
            let render = ToolRenderPart {
                tool: text_field(item, "name"),
                input: field(state, "input").clone(),
                metadata: field(state, "metadata").clone(),
                status: status.clone(),
                output: output.clone(),
            };
            if status == "completed" {
                ctx.render_tool(&ctx.inline_info(&render));
                continue;
            }
            if !output.trim().is_empty() {
                let completed = ToolRenderPart { status: "completed".to_owned(), ..render.clone() };
                ctx.render_tool(&ctx.inline_info(&completed));
            }
            ctx.render_tool_error(&ctx.inline_info(&render));
            let error_message =
                field(state, "error").get("message").and_then(Value::as_str).unwrap_or_default();
            run_ui::error(error_message);
        }

        if let Some(error) = message.get("error").filter(|error| !error.is_null()) {
            let already = {
                let state = ctx.lock();
                state.emitted_error || state.permission_rejected || state.form_cancelled
            };
            if !already {
                let message_text =
                    error.get("message").and_then(Value::as_str).unwrap_or_default().to_owned();
                ctx.lock().emitted_error = true;
                ctx.record_exit(1);
                if !ctx.emit("error", timestamp, &json!({ "error": error })) {
                    run_ui::error(&message_text);
                }
            }
        }
    }
    (found, responded)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kill-switch unit tests mutate the environment; nextest runs each
    /// test in its own process, so there is no cross-test interference.
    #[test]
    fn tty_prompt_enabled_by_default() {
        std::env::remove_var("CI");
        std::env::remove_var("OPENCODE_NO_TTY_PROMPT");
        assert!(permission_tty_prompt_enabled());
    }

    #[test]
    fn ci_env_disables_the_tty_prompt() {
        std::env::remove_var("OPENCODE_NO_TTY_PROMPT");
        std::env::set_var("CI", "1");
        assert!(!permission_tty_prompt_enabled());
        std::env::remove_var("CI");
    }

    #[test]
    fn no_tty_prompt_flag_disables_the_tty_prompt() {
        std::env::remove_var("CI");
        std::env::set_var("OPENCODE_NO_TTY_PROMPT", "1");
        assert!(!permission_tty_prompt_enabled());
        // Any other value keeps the prompt available.
        std::env::set_var("OPENCODE_NO_TTY_PROMPT", "0");
        assert!(permission_tty_prompt_enabled());
        std::env::remove_var("OPENCODE_NO_TTY_PROMPT");
    }
}
