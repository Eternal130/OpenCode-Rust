//! The shared test harness: a deterministic stack (db + bus + projectors)
//! with a fixed model resolver, scripted LLM, stub tools, and the assembled
//! runner.

#![allow(clippy::too_many_lines)]

use std::sync::Arc;
use std::time::Duration;

use oc_ai::schema::options::LanguageModel;
use oc_schema::agent::Info as AgentInfo;
use oc_schema::id::SessionId;
use oc_schema::location;
use oc_util::determinism::{Clock, FixedClock, SequentialIdGen, SequentialRng};

use crate::bus::{Bus, BusOptions};
use crate::database::Database;
use crate::hooks::HookRegistry;
use crate::location::{AgentRegistry, ToolExecute, ToolRegistryState, ToolRunResult};
use crate::session::execution::SessionExecution;
use crate::session::inbox::SessionInbox;
use crate::session::projector::install_projectors;
use crate::session::runner::compaction::SessionCompaction;
use crate::session::runner::context::{ContextDeps, SessionContext};
use crate::session::runner::model_request::{AppMetadata, SessionModelRequest};
use crate::session::runner::title::SessionTitle;
use crate::session::runner::{LlmClient, ModelError, ModelResolver, ResolvedModel, RouteLlmClient};
use crate::session::store::{CreateInput, CreateOutcome, SessionStore};

/// Fixed clock pinned at a known instant.
pub(crate) const NOW_MS: u64 = 1_725_000_000_000;

pub(crate) fn fixed_clock() -> Arc<dyn Clock> {
    Arc::new(FixedClock::new(std::time::UNIX_EPOCH + Duration::from_millis(NOW_MS)))
}

/// `LLMClient` over oc-ai's `TestLlm` — scripted streams.
pub(crate) struct TestLlmClient(pub oc_ai::testing::TestLlm);

#[async_trait::async_trait]
impl LlmClient for TestLlmClient {
    async fn stream(
        &self,
        request: oc_ai::schema::messages::LLMRequest,
    ) -> Result<crate::session::runner::LlmEventStream, oc_ai::schema::AiError> {
        Ok(Box::pin(self.0.stream(request)))
    }

    fn can_compact(
        &self,
        _request: &oc_ai::schema::messages::LLMRequest,
        _mechanism: crate::session::runner::CompactMechanism,
    ) -> bool {
        false
    }

    async fn compact(
        &self,
        _request: oc_ai::schema::messages::LLMRequest,
        _mechanism: crate::session::runner::CompactMechanism,
    ) -> Result<crate::session::runner::CompactOutcome, oc_ai::schema::AiError> {
        Err(oc_ai::schema::AiError::unsupported_operation(
            "compact",
            "TestLlm does not support compaction",
        ))
    }
}

/// The fixed model resolver: one model, optionally the session's own ref.
pub(crate) struct FixedResolver {
    pub resolved: ResolvedModel,
}

impl ModelResolver for FixedResolver {
    fn resolve(
        &self,
        session: &crate::database::schema::SessionRow,
    ) -> Result<ResolvedModel, ModelError> {
        let mut resolved = self.resolved.clone();
        if let Some(selected) = session
            .model
            .as_deref()
            .and_then(|raw| serde_json::from_str::<oc_schema::model::Ref>(raw).ok())
        {
            resolved.reference = selected;
        }
        // Catalog resolution picks a default variant when none is pinned
        // (mirrors v2's `Model.VariantID` default selection).
        if resolved.reference.variant.is_none() {
            resolved.reference.variant = Some(oc_schema::id::ModelVariantId::parse("default"));
        }
        Ok(resolved)
    }

    fn resolve_selected(
        &self,
        session: &crate::database::schema::SessionRow,
        reference: &oc_schema::model::Ref,
    ) -> Result<ResolvedModel, ModelError> {
        let mut resolved = self.resolve(session)?;
        resolved.reference = reference.clone();
        Ok(resolved)
    }
}

pub(crate) fn test_resolved_model() -> ResolvedModel {
    let route =
        oc_ai::test_support::echo_route(Arc::new(oc_ai::route::transport::FailingTransport {
            message: "runner tests never reach a transport",
        }));
    route
        .model("fake-model", Some("parityfake".into()))
        .map(|model| resolved_from(model, None))
        .expect("route model")
}

pub(crate) fn resolved_from(model: LanguageModel, variant: Option<&str>) -> ResolvedModel {
    ResolvedModel {
        reference: oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("parityfake"),
            id: oc_schema::id::ModelId::parse("fake-model"),
            variant: variant.map(oc_schema::id::ModelVariantId::parse),
        },
        model,
        capabilities: oc_schema::model::Capabilities::default(),
        cost: Vec::new(),
        limit: oc_schema::model::ModelLimit { context: 200_000, input: None, output: 32_000 },
        compaction: None,
        transport: None,
    }
}

/// A stub tool executor returning fixed text content.
pub(crate) struct StubTool(pub &'static str);

#[async_trait::async_trait]
impl ToolExecute for StubTool {
    async fn execute(
        &self,
        _input: &serde_json::Value,
    ) -> Result<ToolRunResult, crate::location::ToolRunFailure> {
        Ok(ToolRunResult {
            content: vec![oc_schema::tool::Content::Text { text: self.0.to_string() }],
            metadata: None,
            output: None,
        })
    }
}

/// One assembled runner stack.
pub(crate) struct Harness {
    pub dir: std::path::PathBuf,
    pub db: Database,
    pub bus: Bus,
    pub store: SessionStore,
    pub inbox: SessionInbox,
    pub agents: Arc<AgentRegistry>,
    pub tools: Arc<ToolRegistryState>,
    /// The typed scripted client when the harness was built from a
    /// `TestLlm` (`None` for fully custom clients).
    pub llm: Option<Arc<TestLlmClient>>,
    pub hooks: Arc<HookRegistry>,
    pub compaction: Arc<crate::session::runner::compaction::SessionCompaction>,
    pub title: Arc<SessionTitle>,
    pub runner: Arc<crate::session::runner::drain::SessionRunner>,
    pub execution: SessionExecution,
    pub ids: Arc<SequentialIdGen>,
    _keep: Option<tempfile::TempDir>,
}

pub(crate) fn agent_info(id: &str, steps: Option<u64>) -> AgentInfo {
    AgentInfo {
        id: oc_schema::id::AgentId::parse(id),
        name: oc_schema::id::AgentName::parse(id),
        model: None,
        request: oc_schema::provider::Request::default(),
        system: None,
        description: None,
        mode: if id == "title" || id == "compaction" {
            oc_schema::agent::Mode::Subagent
        } else {
            oc_schema::agent::Mode::Primary
        },
        hidden: false,
        color: None,
        steps,
        permissions: vec![],
    }
}

pub(crate) async fn harness() -> Harness {
    // The default harness answers every request with a plain text turn.
    let llm = oc_ai::testing::TestLlm::new();
    llm.always(oc_ai::testing::text("ok"));
    harness_with(llm).await
}

pub(crate) async fn harness_with(llm: oc_ai::testing::TestLlm) -> Harness {
    harness_with_model(llm, test_resolved_model()).await
}

/// Harness variant with an injected resolved model (e.g. priced models
/// for usage-billing tests).
pub(crate) async fn harness_with_model(
    llm: oc_ai::testing::TestLlm,
    resolved_model: ResolvedModel,
) -> Harness {
    let typed = Arc::new(TestLlmClient(llm));
    let mut harness =
        harness_with_client(std::sync::Arc::clone(&typed) as Arc<dyn LlmClient>, resolved_model)
            .await;
    harness.llm = Some(typed);
    harness
}

/// Harness variant with a fully custom LLM client (e.g. timing-controlled
/// streams for the delta-batching timer tests).
pub(crate) async fn harness_with_client(
    llm: std::sync::Arc<dyn LlmClient>,
    resolved_model: ResolvedModel,
) -> Harness {
    let dir = tempfile::Builder::new()
        .prefix("oc-core-runner-")
        .tempdir_in("/tmp/opencode")
        .expect("temp dir under /tmp/opencode");
    let path = dir.path().to_path_buf();
    let db = Database::open_file(dir.path().join("opencode.db")).await.expect("open db");
    let clock = fixed_clock();
    let options = BusOptions {
        clock: clock.clone(),
        ids: Arc::new(SequentialIdGen::new()),
        persist: true,
        log_read_page_size: 512,
    };
    let bus = Bus::new(db.clone(), options);
    install_projectors(&bus);
    let store = SessionStore::new(db.clone());
    let inbox = SessionInbox::new(db.clone(), bus.clone());

    // Location state: build + title + compaction agents, one stub tool.
    let agents = Arc::new(AgentRegistry::new());
    agents.set(agent_info("build", None));
    agents.set(agent_info("title", None));
    agents.set(agent_info("compaction", None));
    let tools = Arc::new(ToolRegistryState::new());
    tools.register(crate::location::ToolRegistration::new(
        "echo",
        "Echo fixed text",
        serde_json::json!({ "type": "object", "properties": { "text": { "type": "string" } } }),
    ));
    tools.register_executor("echo", Arc::new(StubTool("echo: ok")));

    let hooks = Arc::new(HookRegistry::new());
    let resolver = Arc::new(FixedResolver { resolved: resolved_model });
    let ids = Arc::new(SequentialIdGen::new());

    let directory = path.to_string_lossy().into_owned();
    let environment_dir = directory.clone();
    let deps = ContextDeps::minimal(
        clock.clone(),
        Arc::new(|| Vec::new()),
        Arc::new(move || {
            crate::session::runner::context::environment_block(
                &environment_dir,
                &environment_dir,
                false,
                "/tmp/opencode",
            )
        }),
    );
    let context = Arc::new(SessionContext::new(
        db.clone(),
        bus.clone(),
        crate::location::LocationRef::new(directory.clone(), None),
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
        clock.clone(),
    ));
    let compaction_handle = Arc::clone(&compaction);
    let location_dir = directory.clone();
    let runner = Arc::new(crate::session::runner::drain::SessionRunner::new(
        db.clone(),
        bus.clone(),
        inbox.clone(),
        store.clone(),
        context,
        compaction,
        Arc::clone(&title) as Arc<SessionTitle>,
        Arc::clone(&llm) as Arc<dyn LlmClient>,
        requests,
        clock,
        Arc::new(SequentialRng::new()),
        ids.clone(),
        Arc::new(move || {
            // The harness location never closes mid-test.
            !std::path::Path::new(&location_dir).exists()
        }),
    ));
    let execution =
        SessionExecution::new(store.clone(), bus.clone(), inbox.clone(), None, runner.drain_fn());
    Harness {
        dir: path,
        db,
        bus,
        store,
        inbox,
        agents,
        tools,
        llm: None,
        hooks,
        compaction: compaction_handle,
        title: Arc::clone(&title),
        runner,
        execution,
        ids,
        _keep: Some(dir),
    }
}

impl Harness {
    /// The harness's well-known session id.
    pub(crate) fn session_id(&self) -> SessionId {
        let _ = self;
        SessionId::parse("ses_runner_1").unwrap_or_else(|error| SessionId(error.value))
    }

    pub(crate) async fn create_session(&self) -> crate::database::schema::SessionRow {
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
                    title: Some("Runner test".to_string()),
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
            CreateOutcome::Created(row) | CreateOutcome::Existing(row) => row,
        }
    }

    /// Admits one user prompt (steer) and returns the inbox id.
    pub(crate) async fn admit_prompt(&self, message_id: &str, text: &str) {
        use oc_schema::session_inbox::{Delivery, Item, UserPayload};
        self.inbox
            .admit(
                message_id,
                self.session_id().as_str(),
                Item::User {
                    payload: UserPayload {
                        text: text.to_string(),
                        files: None,
                        agents: None,
                        skills: None,
                        metadata: None,
                    },
                    delivery: Delivery::Steer,
                },
            )
            .await
            .expect("admit prompt");
    }

    /// The committed durable events for the harness session, in seq order.
    pub(crate) async fn durable_events(&self) -> Vec<(String, i64, serde_json::Value)> {
        let mut stream = self.bus.log(crate::bus::LogInput {
            aggregate_id: self.session_id().into(),
            after: None,
            follow: false,
        });
        use futures::StreamExt as _;
        let mut collected = Vec::new();
        while let Some(item) = stream.next().await {
            if let Ok(crate::bus::LogItem::Event(event)) = item {
                if let Some(seq) = event.seq() {
                    collected.push((event.type_.clone(), seq, event.data));
                }
            }
        }
        collected
    }

    /// The projected message tree (`session_message` rows in seq order).
    pub(crate) async fn messages(&self) -> Vec<(String, String, serde_json::Value)> {
        use crate::session::store::{ListOrder, MessagesInput};
        let rows = self
            .store
            .messages(MessagesInput {
                session_id: self.session_id().to_string(),
                order: Some(ListOrder::Asc),
                ..Default::default()
            })
            .await
            .expect("messages");
        rows.into_iter()
            .map(|row| {
                let data: serde_json::Value =
                    serde_json::from_str(&row.data).unwrap_or(serde_json::Value::Null);
                (row.id, row.type_, data)
            })
            .collect()
    }
}

/// Everything the acceptance gate needs from a scripted LLM response with
/// exact usage numbers.
pub(crate) fn parity_text_response(
    text: &str,
    input: f64,
    output: f64,
) -> oc_ai::testing::TestResponse {
    use oc_ai::schema::events::{FinishReason, LLMEvent, Usage};
    let usage = Usage {
        input_tokens: Some(input),
        non_cached_input_tokens: Some(input),
        output_tokens: Some(output),
        reasoning_tokens: Some(0.0),
        total_tokens: Some(input + output),
        ..Default::default()
    };
    let mut response = oc_ai::testing::complete(
        FinishReason::Stop,
        vec![
            LLMEvent::TextStart { id: "text_1".into(), provider_metadata: None },
            LLMEvent::TextDelta { id: "text_1".into(), text: text.into(), provider_metadata: None },
            LLMEvent::TextEnd { id: "text_1".into(), text: None, provider_metadata: None },
        ],
    );
    for event in &mut response.events {
        match event {
            LLMEvent::StepFinish { usage: slot, .. } => *slot = Some(usage.clone()),
            LLMEvent::Finish { usage: slot, .. } => *slot = Some(usage.clone()),
            _ => {}
        }
    }
    response
}

/// `RouteLlmClient` sanity (production client wiring).
#[allow(dead_code)]
fn route_client_marker() -> RouteLlmClient {
    RouteLlmClient
}
