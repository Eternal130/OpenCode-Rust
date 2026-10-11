//! Port of `session/title.ts` (§2.4) — first-promotion title generation
//! with optimistic-concurrency publication.

use oc_ai::schema::events::LLMEvent;
use oc_schema::session_message::Info as MessageInfo;
use oc_util::determinism::Clock;

use std::sync::Arc;

use crate::bus::{Bus, PublishOptions};
use crate::database::Database;
use crate::session::runner::context::{first_user_message, SessionContext};
use crate::session::runner::model_request::{RequestInput, SessionModelRequest};
use crate::session::runner::{LlmClient, RecordedUsage, ResolvedModel, SessionUsage};

const MAX_LENGTH: usize = 100;
const MAX_CONTEXT_LENGTH: usize = 8_000;
const MAX_FIRST_MESSAGE_LENGTH: usize = 2_000;

/// `isUntitled` — a missing title or the exact root fallback for this
/// session (session-title-fallback.ts).
#[must_use]
pub fn is_untitled(session: &crate::database::schema::SessionRow) -> bool {
    let title = session.title.as_deref();
    title.is_none_or(|title| title == fallback_title(session))
}

fn fallback_title(session: &crate::database::schema::SessionRow) -> String {
    format!(
        "{} session - {}",
        if session.parent_id.is_some() { "Child" } else { "New" },
        iso_string(session.time_created)
    )
}

/// `new Date(ms).toISOString()`.
fn iso_string(unix_ms: i64) -> String {
    let ms = unix_ms.rem_euclid(86_400_000);
    let days = unix_ms.div_euclid(86_400_000);
    // Civil-from-days (same algorithm as the date source).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    let hours = ms / 3_600_000;
    let minutes = (ms % 3_600_000) / 60_000;
    let seconds = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{millis:03}Z")
}

/// `truncate` (title.ts:32).
#[must_use]
pub fn truncate(value: &str) -> String {
    if value.chars().count() <= MAX_LENGTH {
        return value.to_string();
    }
    let prefix: String = value.chars().take(MAX_LENGTH - 3).collect();
    format!("{prefix}...")
}

/// The title generation service.
pub struct SessionTitle {
    db: Database,
    bus: Bus,
    llm: Arc<dyn LlmClient>,
    requests: Arc<SessionModelRequest>,
    context: Arc<SessionContext>,
    #[allow(dead_code)]
    clock: Arc<dyn Clock>,
}

impl SessionTitle {
    #[must_use]
    pub fn new(
        db: Database,
        bus: Bus,
        llm: Arc<dyn LlmClient>,
        requests: Arc<SessionModelRequest>,
        context: Arc<SessionContext>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self { db, bus, llm, requests, context, clock }
    }

    /// `attempt` (title.ts:48-98): one model try; any provider error fails
    /// the attempt.
    async fn attempt(
        &self,
        session: &crate::database::schema::SessionRow,
        agent: &oc_schema::agent::Info,
        text: &str,
        model: &ResolvedModel,
    ) -> Option<String> {
        let mut chunks: Vec<String> = Vec::new();
        let mut failed = false;
        let mut usage: Option<RecordedUsage> = None;
        let mut stream = self
            .llm
            .stream(
                self.requests
                    .title(RequestInput {
                        session: session.clone(),
                        agent: agent.id.clone(),
                        model: model.clone(),
                        tools: None,
                        system: agent
                            .system
                            .as_ref()
                            .map(|system| {
                                vec![oc_ai::schema::messages::SystemPart::new(system.clone())]
                            })
                            .unwrap_or_default(),
                        messages: vec![oc_ai::schema::messages::Message::user(text)],
                        tool_choice: None,
                        web_socket: false,
                        input_tokens: None,
                    })
                    .await
                    .ok()?
                    .request,
            )
            .await
            .ok()?;
        use futures::StreamExt;
        while let Some(event) = stream.next().await {
            let Ok(event) = event else {
                failed = true;
                break;
            };
            match &event {
                LLMEvent::ProviderError { .. } => failed = true,
                LLMEvent::TextDelta { text, .. } => chunks.push(text.clone()),
                LLMEvent::StepFinish { usage: step_usage, .. } => {
                    let step = SessionUsage::record(step_usage.as_ref(), &model.cost);
                    usage = Some(match usage {
                        Some(existing) => SessionUsage::add(&existing, &step),
                        None => step,
                    });
                }
                _ => {}
            }
        }
        if let Some(recorded) = usage {
            let data = serde_json::json!({
                "sessionID": session.id,
                "source": "title",
                "cost": recorded.cost.0 .0,
                "tokens": crate::session::runner::tokens_json(&recorded.tokens),
            });
            let def = crate::bus::registry::by_type_latest("session.usage.recorded")
                .unwrap_or_else(|| panic!("no session.usage.recorded definition"));
            let _ = self.bus.publish(def, data, PublishOptions::default()).await;
        }
        if failed {
            return None;
        }
        chunks.join("").split('\n').map(str::trim).find(|line| !line.is_empty()).map(str::to_string)
    }

    /// `generate` (title.ts:100-148).
    pub async fn generate(&self, session_id: &str) {
        let Some(session) = self
            .db
            .transaction({
                let id = session_id.to_string();
                move |tx| crate::database::schema::SessionRow::get(tx, id)
            })
            .await
            .ok()
            .flatten()
        else {
            return;
        };
        let Some(first_user) = first_user_message(&self.db, session_id).await else { return };
        let MessageInfo::User { id: first_user_id, text: first_user_text, .. } = &first_user else {
            return;
        };
        let text = if is_untitled(&session) {
            first_user_text.clone()
        } else {
            match self.context_messages(session_id).await {
                Ok(messages) => {
                    let original = format!(
                        "Original request:\n{}",
                        truncate_chars(first_user_text, MAX_FIRST_MESSAGE_LENGTH)
                    );
                    let recent: Vec<String> = messages
                        .iter()
                        .filter_map(|message| match message {
                            MessageInfo::User { id, text, .. } if id != first_user_id => {
                                Some(format!("User: {}", text.trim()))
                            }
                            MessageInfo::Assistant { content, .. } => {
                                let text: Vec<String> = content
                                    .iter()
                                    .filter_map(|part| match part {
                                        oc_schema::session_message::AssistantContent::Text {
                                            text,
                                            ..
                                        } => Some(text.trim().to_string()),
                                        _ => None,
                                    })
                                    .filter(|text| !text.is_empty())
                                    .collect();
                                if text.is_empty() {
                                    None
                                } else {
                                    Some(format!("Assistant: {}", text.join("\n")))
                                }
                            }
                            _ => None,
                        })
                        .collect();
                    let recent = recent.join("\n\n");
                    if recent.is_empty() {
                        original
                    } else {
                        let prefix = format!("{original}\n\nRecent conversation:\n");
                        format!(
                            "{prefix}{}",
                            truncate_chars_from(
                                &recent,
                                MAX_CONTEXT_LENGTH.saturating_sub(prefix.len())
                            )
                        )
                    }
                }
                Err(_) => first_user_text.clone(),
            }
        };
        let Some((agent, primary, selected)) = self.select_title(&session) else { return };
        let mut title = self.attempt(&session, &agent, &text, &selected).await;
        if title.is_none() {
            if let Some(primary) = primary.clone() {
                if primary.reference != selected.reference {
                    title = self.attempt(&session, &agent, &text, &primary).await;
                }
            }
        }
        let Some(title) = title else { return };

        // Optimistic concurrency: the renamed publish lands only at the
        // expected next sequence (title.ts:135-147). The forked title task
        // races the chat step's own session events — any of them
        // allocating a sequence between the expectation read and the
        // publish loses the rename — so a mismatch retries (fresh
        // expectation, fresh current-row guard, small backoff for the
        // concurrent stream's event burst). A concurrent RENAME still
        // wins: the current-row re-check below returns when the title
        // moved (the same guard v2's pre-publish check applies).
        for attempt in 0..20 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            let expected_sequence =
                self.bus.latest_sequence(session_id).await.map(|seq| seq + 1).unwrap_or(1);
            let Some(current) = self
                .db
                .transaction({
                    let id = session_id.to_string();
                    move |tx| crate::database::schema::SessionRow::get(tx, id)
                })
                .await
                .ok()
                .flatten()
            else {
                return;
            };
            if current.title != session.title
                || current.title.as_deref() == Some(truncate(&title).as_str())
            {
                return;
            }
            let mut options = PublishOptions::default();
            let session_id_owned = session_id.to_string();
            options.commit = Some(Box::new(move |_tx: &mut crate::database::Tx<'_>, seq: i64| {
                if seq == expected_sequence {
                    Ok(())
                } else {
                    Err(crate::database::error::DbError::Pragma(format!(
                        "\u{0}title-changed\u{1}{session_id_owned}"
                    )))
                }
            }));
            let data = serde_json::json!({
                "sessionID": session_id,
                "title": truncate(&title),
            });
            let def = crate::bus::registry::by_type_latest("session.renamed")
                .unwrap_or_else(|| panic!("no session.renamed definition"));
            match self.bus.publish(def, data, options).await {
                Ok(_) => return,
                // A lost race is swallowed (the defect only signals the
                // mismatch) and retried with a fresh expectation.
                Err(error) if Self::is_title_changed(&error) => continue,
                Err(_) => return,
            }
        }
    }

    /// Whether a rename-publish failure is the optimistic-concurrency
    /// defect (a sequence mismatch) rather than a hard bus error.
    fn is_title_changed(error: &crate::bus::BusError) -> bool {
        matches!(
            error,
            crate::bus::BusError::Db(crate::database::error::DbError::Pragma(message))
                if message.starts_with("\u{0}title-changed")
        )
    }

    /// `selectTitle` (context.ts:95-119) — aux title capabilities without
    /// instruction or tool preflight.
    fn select_title(
        &self,
        session: &crate::database::schema::SessionRow,
    ) -> Option<(oc_schema::agent::Info, Option<ResolvedModel>, ResolvedModel)> {
        self.context.select_title(session)
    }

    async fn context_messages(
        &self,
        session_id: &str,
    ) -> crate::database::error::Result<Vec<MessageInfo>> {
        crate::session::runner::context::history_load(
            &self.db,
            session_id,
            &crate::session::runner::context::Boundary::Latest,
        )
        .await
    }
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect()
}

fn truncate_chars_from(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let start = value.chars().count() - max;
    value.chars().skip(start).collect()
}
