//! RUNTIME cluster regressions (parity ledger): the production prompt
//! path's `SessionPrompt.prepare` seam — `session.prompt` hook dispatch
//! (RUNTIME-P01) and `@skill` attachment resolution (RUNTIME-P02).
//!
//! The stack below is the REAL production assembly shape (db + bus +
//! projectors + inbox + store + runner + execution), built through
//! public API only: the [`PromptPrepare`] seam attaches the location's
//! hook registry and skill table to the runner exactly as the engine
//! does (`Engine::assemble` → `SessionRunner::with_prompt_prepare`),
//! then the tests drive the full prompt path —
//! `SessionInbox::admit` → `SessionExecution::resume` → drain → Ready
//! boundary → prepare → deliver → model turn — and assert BOTH
//! observable effects: the scripted LLM's captured request (the model
//! turn) and the projected `session_message` row (the stored user
//! message).
//!
//! Hermetic: sandbox under /tmp/opencode, no network (the LLM is
//! `oc_ai::testing::TestLlm` over a failing transport), fixed clock +
//! sequential ids.

use std::sync::Arc;

use oc_ai::schema::messages::LLMRequest;
use oc_core::database::Database;
use oc_core::hooks::{fn_hook, HookError, HookOptions, HookRegistry, SessionPromptEvent};
use oc_core::session::execution::SessionExecution;
use oc_core::session::inbox::SessionInbox;
use oc_core::session::projector::install_projectors;
use oc_core::session::runner::compaction::SessionCompaction;
use oc_core::session::runner::context::{environment_block, ContextDeps, SessionContext};
use oc_core::session::runner::drain::SessionRunner;
use oc_core::session::runner::model_request::{AppMetadata, SessionModelRequest};
use oc_core::session::runner::prompt_prepare::PromptPrepare;
use oc_core::session::runner::title::SessionTitle;
use oc_core::session::runner::{LlmClient, ModelError, ModelResolver, ResolvedModel};
use oc_core::session::store::{CreateInput, CreateOutcome, MessagesInput, SessionStore};
use oc_core::tool::skill::MemorySkillTable;
use oc_schema::agent::{Info as AgentInfo, Mode};
use oc_schema::id::SessionId;
use oc_schema::location;
use oc_schema::prompt::SkillAttachment;
use oc_schema::session_inbox::{Delivery, Item, UserPayload};
use oc_schema::skill::Skill;
use oc_util::determinism::{Clock, FixedClock, SequentialIdGen, SequentialRng};

const NOW_MS: u64 = 1_725_000_000_000;

// ---------------------------------------------------------------------------
// The stack (public-API mirror of the engine's runner assembly)
// ---------------------------------------------------------------------------

struct TestLlmClient(oc_ai::testing::TestLlm);

#[async_trait::async_trait]
impl LlmClient for TestLlmClient {
    async fn stream(
        &self,
        request: LLMRequest,
    ) -> Result<oc_core::session::runner::LlmEventStream, oc_ai::schema::AiError> {
        Ok(Box::pin(self.0.stream(request)))
    }

    fn can_compact(
        &self,
        _request: &LLMRequest,
        _mechanism: oc_core::session::runner::CompactMechanism,
    ) -> bool {
        false
    }

    async fn compact(
        &self,
        _request: LLMRequest,
        _mechanism: oc_core::session::runner::CompactMechanism,
    ) -> Result<oc_core::session::runner::CompactOutcome, oc_ai::schema::AiError> {
        Err(oc_ai::schema::AiError::unsupported_operation(
            "compact",
            "TestLlm does not support compaction",
        ))
    }
}

struct FixedResolver {
    resolved: ResolvedModel,
}

impl ModelResolver for FixedResolver {
    fn resolve(
        &self,
        session: &oc_core::database::schema::SessionRow,
    ) -> Result<ResolvedModel, ModelError> {
        let mut resolved = self.resolved.clone();
        if let Some(selected) = session
            .model
            .as_deref()
            .and_then(|raw| serde_json::from_str::<oc_schema::model::Ref>(raw).ok())
        {
            resolved.reference = selected;
        }
        if resolved.reference.variant.is_none() {
            resolved.reference.variant = Some(oc_schema::id::ModelVariantId::parse("default"));
        }
        Ok(resolved)
    }

    fn resolve_selected(
        &self,
        session: &oc_core::database::schema::SessionRow,
        reference: &oc_schema::model::Ref,
    ) -> Result<ResolvedModel, ModelError> {
        let mut resolved = self.resolve(session)?;
        resolved.reference = reference.clone();
        Ok(resolved)
    }
}

fn test_resolved_model() -> ResolvedModel {
    let route =
        oc_ai::test_support::echo_route(Arc::new(oc_ai::route::transport::FailingTransport {
            message: "runner tests never reach a transport",
        }));
    let model = route.model("fake-model", Some("parityfake".into())).expect("route model");
    ResolvedModel {
        reference: oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("parityfake"),
            id: oc_schema::id::ModelId::parse("fake-model"),
            variant: Some(oc_schema::id::ModelVariantId::parse("default")),
        },
        model,
        capabilities: oc_schema::model::Capabilities::default(),
        cost: Vec::new(),
        limit: oc_schema::model::ModelLimit { context: 200_000, input: None, output: 32_000 },
        compaction: None,
        transport: None,
    }
}

fn agent_info(id: &str) -> AgentInfo {
    AgentInfo {
        id: oc_schema::id::AgentId::parse(id),
        name: oc_schema::id::AgentName::parse(id),
        model: None,
        request: oc_schema::provider::Request::default(),
        system: None,
        description: None,
        mode: if id == "title" || id == "compaction" { Mode::Subagent } else { Mode::Primary },
        hidden: false,
        color: None,
        steps: None,
        permissions: vec![],
    }
}

struct Stack {
    dir: std::path::PathBuf,
    bus: oc_core::bus::Bus,
    hooks: Arc<HookRegistry>,
    inbox: SessionInbox,
    store: SessionStore,
    llm: Arc<TestLlmClient>,
    execution: SessionExecution,
    _keep: tempfile::TempDir,
}

async fn stack(llm: oc_ai::testing::TestLlm) -> Stack {
    stack_with(llm, MemorySkillTable::default()).await
}

async fn stack_with(llm: oc_ai::testing::TestLlm, skills: MemorySkillTable) -> Stack {
    let dir = tempfile::Builder::new()
        .prefix("oc-core-runtime-")
        .tempdir_in("/tmp/opencode")
        .expect("sandbox under /tmp/opencode");
    let path = dir.path().to_path_buf();
    let db = Database::open_file(dir.path().join("opencode.db")).await.expect("open db");
    let clock: Arc<dyn Clock> =
        Arc::new(FixedClock::new(std::time::UNIX_EPOCH + std::time::Duration::from_millis(NOW_MS)));
    let bus = oc_core::bus::Bus::new(
        db.clone(),
        oc_core::bus::BusOptions {
            clock: clock.clone(),
            ids: Arc::new(SequentialIdGen::new()),
            persist: true,
            log_read_page_size: 512,
        },
    );
    install_projectors(&bus);
    let store = SessionStore::new(db.clone());
    let inbox = SessionInbox::new(db.clone(), bus.clone());

    let agents = Arc::new(oc_core::location::AgentRegistry::new());
    agents.set(agent_info("build"));
    agents.set(agent_info("title"));
    agents.set(agent_info("compaction"));
    let tools = Arc::new(oc_core::location::ToolRegistryState::new());

    let hooks = Arc::new(HookRegistry::new());
    let llm = Arc::new(TestLlmClient(llm));
    let resolver = Arc::new(FixedResolver { resolved: test_resolved_model() });
    let ids = Arc::new(SequentialIdGen::new());

    let environment_dir = path.to_string_lossy().into_owned();
    let deps = ContextDeps::minimal(
        clock.clone(),
        Arc::new(Vec::new),
        Arc::new(move || {
            environment_block(&environment_dir, &environment_dir, false, "/tmp/opencode")
        }),
    );
    let context = Arc::new(SessionContext::new(
        db.clone(),
        bus.clone(),
        oc_core::location::LocationRef::new(path.to_string_lossy().into_owned(), None),
        Arc::clone(&agents),
        Arc::clone(&tools),
        resolver.clone(),
        deps,
    ));
    let requests = Arc::new(SessionModelRequest::new(
        Arc::clone(&hooks),
        Arc::clone(&tools),
        AppMetadata::default(),
    ));
    let compaction = Arc::new(SessionCompaction::new(
        db.clone(),
        bus.clone(),
        Arc::clone(&llm) as Arc<dyn LlmClient>,
        Arc::clone(&requests),
        Arc::clone(&agents),
        resolver,
        Arc::new(SequentialRng::new()),
    ));
    let title = Arc::new(SessionTitle::new(
        db.clone(),
        bus.clone(),
        Arc::clone(&llm) as Arc<dyn LlmClient>,
        Arc::clone(&requests),
        Arc::clone(&context),
        clock,
    ));

    // The production seam shape: the engine attaches the location's
    // hook registry + skill table (Engine::assemble → with_prompt_prepare).
    let skills = Arc::new(skills);
    let prepare = Arc::new(PromptPrepare::new(
        db.clone(),
        Arc::clone(&hooks),
        Arc::clone(&skills) as Arc<dyn oc_core::tool::skill::SkillTable>,
    ));
    let location_dir = path.to_string_lossy().into_owned();
    let runner = Arc::new(
        SessionRunner::new(
            db.clone(),
            bus.clone(),
            inbox.clone(),
            store.clone(),
            context,
            compaction,
            title,
            Arc::clone(&llm) as Arc<dyn LlmClient>,
            requests,
            Arc::new(FixedClock::new(
                std::time::UNIX_EPOCH + std::time::Duration::from_millis(NOW_MS),
            )),
            Arc::new(SequentialRng::new()),
            ids.clone(),
            Arc::new(move || !std::path::Path::new(&location_dir).exists()),
        )
        .with_prompt_prepare(prepare),
    );
    let execution =
        SessionExecution::new(store.clone(), bus.clone(), inbox.clone(), None, runner.drain_fn());
    Stack { dir: path, bus, hooks, inbox, store, llm, execution, _keep: dir }
}

impl Stack {
    fn session_id(&self) -> SessionId {
        let _ = self;
        SessionId::parse("ses_runner_1").unwrap_or_else(|error| SessionId(error.value))
    }

    async fn create_session(&self) {
        let location = location::Ref {
            directory: oc_schema::common::AbsolutePath::parse(
                self.dir.to_string_lossy().into_owned(),
            ),
            workspace_id: None,
        };
        match self
            .store
            .create(
                &self.bus,
                CreateInput {
                    id: Some(self.session_id()),
                    location: Some(location),
                    title: Some("Runtime test".to_string()),
                    agent: None,
                    model: Some(oc_schema::model::Ref {
                        provider_id: oc_schema::id::ProviderId::parse("parityfake"),
                        id: oc_schema::id::ModelId::parse("fake-model"),
                        variant: Some(oc_schema::id::ModelVariantId::parse("default")),
                    }),
                    permissions: None,
                    metadata: None,
                    parent_id: None,
                    project: None,
                    version: "test".to_string(),
                },
            )
            .await
            .expect("create session")
        {
            CreateOutcome::Created(_) | CreateOutcome::Existing(_) => {}
        }
    }

    async fn admit(&self, message_id: &str, payload: UserPayload) {
        self.inbox
            .admit(
                message_id,
                self.session_id().as_str(),
                Item::User { payload, delivery: Delivery::Steer },
            )
            .await
            .expect("admit prompt");
    }

    /// The projected user message's JSON row data.
    async fn user_message_data(&self) -> serde_json::Value {
        let rows = self
            .store
            .messages(MessagesInput {
                session_id: self.session_id().to_string(),
                order: Some(oc_core::session::store::ListOrder::Asc),
                ..Default::default()
            })
            .await
            .expect("messages");
        let row = rows
            .iter()
            .find(|row| row.type_ == "user")
            .unwrap_or_else(|| panic!("no user message: {rows:?}"));
        serde_json::from_str(&row.data).unwrap_or(serde_json::Value::Null)
    }

    /// The concatenated user-role text of the first captured LLM request.
    fn llm_user_text(&self) -> String {
        use oc_ai::schema::messages::ContentPart;
        let requests = self.llm.0.requests();
        let request = requests.first().expect("a model request happened");
        request
            .messages
            .iter()
            .filter(|message| message.role == oc_ai::schema::messages::MessageRole::User)
            .flat_map(|message| message.content.iter())
            .map(|part| match part {
                ContentPart::Text { text, .. } => text.clone(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ---------------------------------------------------------------------------
// RUNTIME-P01: session.prompt dispatches on the production prompt path
// ---------------------------------------------------------------------------

/// A registered `session.prompt` hook rewrites the text; BOTH the
/// model turn (the captured request) and the stored user message
/// carry the rewritten prompt (copy-in/copy-out per ADR-0005).
#[tokio::test]
async fn prompt_hook_rewrites_message() {
    let llm = oc_ai::testing::TestLlm::new();
    llm.always(oc_ai::testing::text("ok"));
    let stack = stack(llm).await;
    // The registration guard outlives the drain.
    let _guard = stack
        .hooks
        .register_session_prompt(
            fn_hook(|event: &mut SessionPromptEvent| {
                event.prompt.text = format!("[hooked] {}", event.prompt.text);
                Ok(())
            }),
            HookOptions::default(),
        )
        .await;

    stack.create_session().await;
    stack
        .admit("msg_user_1", UserPayload { text: "original prompt".into(), ..Default::default() })
        .await;
    stack.execution.resume(stack.session_id().as_str()).await.expect("execution exit");

    // The model turn saw the rewritten prompt.
    assert_eq!(stack.llm_user_text(), "[hooked] original prompt");
    // The stored user message row carries the rewritten prompt too.
    let data = stack.user_message_data().await;
    assert_eq!(data["text"], serde_json::json!("[hooked] original prompt"));
}

/// A hook that mutates metadata sees it persisted (the metadata
/// copy-out channel).
#[tokio::test]
async fn prompt_hook_metadata_copy_out() {
    let llm = oc_ai::testing::TestLlm::new();
    llm.always(oc_ai::testing::text("ok"));
    let stack = stack(llm).await;
    let _guard = stack
        .hooks
        .register_session_prompt(
            fn_hook(|event: &mut SessionPromptEvent| {
                event
                    .metadata
                    .get_or_insert_with(Default::default)
                    .insert("source".into(), serde_json::json!("hook"));
                Ok(())
            }),
            HookOptions::default(),
        )
        .await;

    stack.create_session().await;
    stack.admit("msg_user_1", UserPayload { text: "hello".into(), ..Default::default() }).await;
    stack.execution.resume(stack.session_id().as_str()).await.expect("execution exit");

    let data = stack.user_message_data().await;
    assert_eq!(data["metadata"]["source"], serde_json::json!("hook"));
}

/// A rejecting hook fails the prompt (surfaced error, the poisoned row
/// cancelled — v2 never admits a prompt whose prepare failed).
#[tokio::test]
async fn prompt_hook_rejection_fails_the_prompt() {
    let llm = oc_ai::testing::TestLlm::new();
    llm.always(oc_ai::testing::text("ok"));
    let stack = stack(llm).await;
    let _guard = stack
        .hooks
        .register_session_prompt(
            fn_hook(|_event: &mut SessionPromptEvent| Err(HookError::Handler("redacted".into()))),
            HookOptions::default(),
        )
        .await;

    stack.create_session().await;
    stack
        .admit("msg_user_1", UserPayload { text: "never runs".into(), ..Default::default() })
        .await;
    let outcome = stack.execution.resume(stack.session_id().as_str()).await;
    let error = outcome.expect_err("a rejected hook fails the prompt");
    assert_eq!(error.to_string(), "drain failed: prompt.failed");
    assert!(
        error.to_session_error().message.contains("redacted"),
        "the hook error surfaces in the session error: {error:?}"
    );
    // The poisoned row was cancelled (v2 never admits it).
    let pending = stack.inbox.list(stack.session_id().as_str()).await.expect("list");
    assert!(pending.is_empty(), "the rejected prompt is gone: {pending:?}");
    // And no model request ever happened.
    assert!(stack.llm.0.requests().is_empty(), "no model turn for a rejected prompt");
}

// ---------------------------------------------------------------------------
// RUNTIME-P02: @skill attachments resolve on the production path
// ---------------------------------------------------------------------------

fn skill_in_sandbox(tag: &str) -> (Skill, tempfile::TempDir) {
    let dir = tempfile::Builder::new()
        .prefix(&format!("oc-core-skill-{tag}-"))
        .tempdir_in("/tmp/opencode")
        .expect("skill sandbox under /tmp/opencode");
    let skill_dir = dir.path().join("deploy-helper");
    std::fs::create_dir_all(&skill_dir).expect("skill dir");
    std::fs::write(skill_dir.join("SKILL.md"), "Deploy the service carefully.").expect("SKILL.md");
    std::fs::write(skill_dir.join("runbook.txt"), "step 1: drain\nstep 2: restart")
        .expect("runbook");
    (
        Skill {
            id: oc_schema::id::SkillId::parse("deploy-helper"),
            name: oc_schema::id::SkillName::parse("deploy-helper"),
            description: Some("Deployment runbook".into()),
            auto_invoke: None,
            path: oc_schema::common::AbsolutePath::parse(
                skill_dir.join("SKILL.md").to_string_lossy().into_owned(),
            ),
            content: "Deploy the service carefully.".into(),
        },
        dir,
    )
}

/// A prompt-attached `@skill` resolves to the enveloped skill body:
/// the model turn and the stored message both carry the
/// `<skill_content>` text (v2 `Skill.prepare().output`), ahead of the
/// prompt text.
#[tokio::test]
async fn prompt_skill_prepare() {
    let llm = oc_ai::testing::TestLlm::new();
    llm.always(oc_ai::testing::text("ok"));
    let (skill, _skill_keep) = skill_in_sandbox("prepare");
    let mut table = MemorySkillTable::default();
    table.skills.push(skill.clone());
    let stack = stack_with(llm, table).await;

    stack.create_session().await;
    stack
        .admit(
            "msg_user_1",
            UserPayload {
                text: "deploy now".into(),
                skills: Some(vec![SkillAttachment {
                    id: skill.id.clone(),
                    name: skill.name.clone(),
                    text: None,
                    mention: None,
                }]),
                ..Default::default()
            },
        )
        .await;
    stack.execution.resume(stack.session_id().as_str()).await.expect("execution exit");

    let text = stack.llm_user_text();
    assert!(
        text.starts_with("<skill_content name=\"deploy-helper\">"),
        "the skill body leads the user content: {text}"
    );
    assert!(text.contains("Deploy the service carefully."), "skill content injected: {text}");
    assert!(text.contains("<file>"), "the sampled file list rides the envelope: {text}");
    assert!(text.ends_with("deploy now"), "the prompt text follows the skill body: {text}");
    // The stored message row carries the resolved attachment.
    let data = stack.user_message_data().await;
    let stored_skill = &data["skills"][0];
    assert_eq!(stored_skill["name"], serde_json::json!("deploy-helper"));
    let stored_text = stored_skill["text"].as_str().unwrap_or_default();
    assert!(
        stored_text.starts_with("<skill_content name=\"deploy-helper\">"),
        "the row's skill.text is the prepared body: {stored_text}"
    );
}

/// An unknown skill id fails the prompt with an error (never a silent
/// no-op) and cancels the poisoned row.
#[tokio::test]
async fn prompt_unknown_skill_errors() {
    let llm = oc_ai::testing::TestLlm::new();
    llm.always(oc_ai::testing::text("ok"));
    let stack = stack(llm).await;
    stack.create_session().await;
    stack
        .admit(
            "msg_user_1",
            UserPayload {
                text: "use the ghost skill".into(),
                skills: Some(vec![SkillAttachment {
                    id: oc_schema::id::SkillId::parse("ghost-skill"),
                    name: oc_schema::id::SkillName::parse("ghost-skill"),
                    text: None,
                    mention: None,
                }]),
                ..Default::default()
            },
        )
        .await;
    let outcome = stack.execution.resume(stack.session_id().as_str()).await;
    let error = outcome.expect_err("an unknown skill fails the prompt");
    assert_eq!(error.to_string(), "drain failed: prompt.failed");
    assert!(
        error.to_session_error().message.contains("ghost-skill"),
        "the error names the unknown skill: {error:?}"
    );
    let pending = stack.inbox.list(stack.session_id().as_str()).await.expect("list");
    assert!(pending.is_empty(), "the poisoned prompt is cancelled: {pending:?}");
    assert!(stack.llm.0.requests().is_empty(), "no model turn for a failed prepare");
}

/// A skill-less prompt passes through the armed seam untouched (the
/// no-attachment fast path — no skill resolution, unchanged text).
#[tokio::test]
async fn prompt_prepare_skillless_passthrough() {
    let llm = oc_ai::testing::TestLlm::new();
    llm.always(oc_ai::testing::text("ok"));
    let (skill, _keep) = skill_in_sandbox("passthrough");
    let mut table = MemorySkillTable::default();
    table.skills.push(skill);
    let stack = stack_with(llm, table).await;
    stack.create_session().await;
    stack
        .admit(
            "msg_user_1",
            UserPayload {
                text: "plain prompt".into(),
                skills: Some(Vec::new()),
                ..Default::default()
            },
        )
        .await;
    stack.execution.resume(stack.session_id().as_str()).await.expect("execution exit");
    assert_eq!(stack.llm_user_text(), "plain prompt");
}
