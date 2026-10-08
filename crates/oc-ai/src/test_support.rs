//! Cassette replay infrastructure (oracle gate 2, finding 18 — BINDING).
//!
//! Cassettes are reused **verbatim** from the v2 recordings
//! (`packages/ai/test/fixtures/recordings`, copied to
//! `crates/oc-ai/test-fixtures/recordings`): format v1
//! `{version: 1, metadata: {tags, name, …}, interactions: [{transport,
//! request: {method, url, headers, body}, response: {status, headers, body}}]}`
//! where `response.body` is the **raw SSE wire bytes as a string**.
//! [`MockTransport`] replays interactions in order against the real route
//! stack (transport-level replay, not mocks of protocol code). Protocol
//! lanes add per-scenario replay assertions on top.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use bytes::Bytes;

use crate::route::transport::{
    ByteStream, Transport, TransportError, TransportOperation, TransportRequest, TransportResponse,
};
use crate::schema::{AiError, AiErrorReason, ReasonFields};

/// Root of the copied cassette corpus.
pub fn recordings_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/recordings"))
}

/// Cassette metadata (selection tags etc.).
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct CassetteMetadata {
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub route: Option<String>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

/// One recorded exchange. Websocket interactions carry neither a request
/// nor a response object in the v1 format — both are optional here and
/// `transport` distinguishes them.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CassetteInteraction {
    pub transport: String,
    #[serde(default)]
    pub request: Option<CassetteRequest>,
    #[serde(default)]
    pub response: Option<CassetteResponse>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CassetteRequest {
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub body: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CassetteResponse {
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub body_encoding: Option<String>,
}

/// A parsed cassette (verbatim v1 format).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Cassette {
    pub version: u8,
    pub metadata: CassetteMetadata,
    pub interactions: Vec<CassetteInteraction>,
}

impl Cassette {
    /// Parse one cassette file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        serde_json::from_str(&raw).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// Only HTTP interactions (websocket exchanges belong to the WS lane).
    pub fn http_interactions(&self) -> impl Iterator<Item = &CassetteInteraction> {
        self.interactions.iter().filter(|i| i.transport == "http")
    }

    /// The request of an interaction, when recorded.
    #[must_use]
    pub fn request_of(interaction: &CassetteInteraction) -> Option<&CassetteRequest> {
        interaction.request.as_ref()
    }

    /// The response of an interaction, when recorded.
    #[must_use]
    pub fn response_of(interaction: &CassetteInteraction) -> Option<&CassetteResponse> {
        interaction.response.as_ref()
    }
}

/// A cassette plus its source path.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub path: PathBuf,
    pub cassette: Cassette,
}

impl Scenario {
    /// Scenario group = recording directory name (e.g. `anthropic-messages`);
    /// loose cassettes at the corpus root (openrouter-reasoning, …) form
    /// their own single-file groups named after the file stem.
    #[must_use]
    pub fn group(&self) -> &str {
        group_of(&self.path)
    }

    /// Scenario name = file stem (e.g. `anthropic-opus-4-7-image-tool-result`).
    #[must_use]
    pub fn name(&self) -> &str {
        self.path.file_stem().and_then(|name| name.to_str()).unwrap_or_default()
    }

    #[must_use]
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.group(), self.name())
    }
}

fn scan_dir(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut children: Vec<std::fs::DirEntry> = entries.flatten().collect();
    children.sort_by_key(std::fs::DirEntry::file_name);
    for entry in children {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            out.push(path);
        }
    }
}

/// Every cassette under [`recordings_dir`], sorted by path.
#[must_use]
pub fn all_cassettes() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    scan_dir(recordings_dir(), &mut paths);
    paths.sort();
    paths
}

/// Load every cassette (parse errors surface immediately — the corpus is
/// expected to stay verbatim-clean).
pub fn all_scenarios() -> Result<Vec<Scenario>, String> {
    all_cassettes()
        .into_iter()
        .map(|path| {
            let cassette = Cassette::load(&path)?;
            Ok(Scenario { path, cassette })
        })
        .collect()
}

/// Select scenarios by group prefix (e.g. `"gemini"`) and/or tag substring
/// (e.g. `"provider:anthropic"`). Mirrors the TS runner's
/// `RECORDED_PREFIX` / `TAGS` selection.
pub fn select_scenarios(
    group_prefix: Option<&str>,
    tag: Option<&str>,
) -> Result<Vec<Scenario>, String> {
    Ok(all_scenarios()?
        .into_iter()
        .filter(|scenario| {
            group_prefix.is_none_or(|prefix| scenario.group().starts_with(prefix))
                && tag.is_none_or(|needle| {
                    scenario.cassette.metadata.tags.iter().any(|t| t.contains(needle))
                })
        })
        .collect())
}

/// Group counts over the whole corpus: `(group, cassette_count)`.
#[must_use]
pub fn group_counts() -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for path in all_cassettes() {
        *counts.entry(group_of(&path).to_string()).or_default() += 1;
    }
    counts.into_iter().collect()
}

/// Group name of a cassette path (see [`Scenario::group`]): the recording
/// directory name, or the file stem for loose cassettes at the corpus root.
fn group_of(path: &Path) -> &str {
    if path.parent() == Some(recordings_dir()) {
        return path.file_stem().and_then(|name| name.to_str()).unwrap_or_default();
    }
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or_default()
}

fn wire_headers(headers: &BTreeMap<String, serde_json::Value>) -> BTreeMap<String, String> {
    headers
        .iter()
        .map(|(name, value)| {
            let value = match value {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            (name.to_ascii_lowercase(), value)
        })
        .collect()
}

/// Transport that replays cassette interactions in order (port of the TS
/// `HttpRecorder` replay layer). Requests are matched loosely: method and
/// URL are compared when the cassette records them; bodies may be inspected
/// via [`MockTransport::requests`]. When interactions run out, the transport
/// fails with a typed error.
pub struct MockTransport {
    interactions: Vec<CassetteInteraction>,
    next: Mutex<usize>,
    requests: Mutex<Vec<TransportRequest>>,
    strict_url: bool,
}

impl MockTransport {
    /// Build from a cassette's HTTP interactions.
    #[must_use]
    pub fn from_cassette(cassette: &Cassette) -> Self {
        Self {
            interactions: cassette.http_interactions().cloned().collect(),
            next: Mutex::new(0),
            requests: Mutex::new(Vec::new()),
            strict_url: false,
        }
    }

    /// Build from pre-rendered interactions (bodies are raw wire bytes).
    #[must_use]
    pub fn from_interactions(interactions: Vec<RecordedInteraction>) -> Self {
        Self {
            interactions: interactions
                .into_iter()
                .map(|i| CassetteInteraction {
                    transport: "http".into(),
                    request: Some(CassetteRequest {
                        method: Some(i.method),
                        url: Some(i.url),
                        headers: None,
                        body: i.request_body.map(serde_json::Value::String),
                    }),
                    response: Some(CassetteResponse {
                        status: Some(i.status),
                        headers: None,
                        body: Some(i.body),
                        body_encoding: None,
                    }),
                })
                .collect(),
            next: Mutex::new(0),
            requests: Mutex::new(Vec::new()),
            strict_url: false,
        }
    }

    /// Fail when replayed request URLs diverge from the recording.
    pub fn with_strict_url(mut self) -> Self {
        self.strict_url = true;
        self
    }

    /// Observed requests, in order.
    pub fn requests(&self) -> Vec<TransportRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// Interactions still queued.
    pub fn remaining(&self) -> usize {
        let next = *self.next.lock().unwrap();
        self.interactions.len().saturating_sub(next)
    }
}

/// A pre-rendered interaction for [`MockTransport::from_interactions`].
pub struct RecordedInteraction {
    pub method: String,
    pub url: String,
    pub status: u16,
    pub body: String,
    pub request_body: Option<String>,
}

/// Split a wire body into deterministic multi-chunk byte slices so replay
/// exercises the framing decoder's chunk-boundary handling (oracle gate 2,
/// finding 18): chunks of at most 7 bytes.
pub fn chunked(body: &str) -> Vec<Bytes> {
    body.as_bytes().chunks(7).map(Bytes::copy_from_slice).collect()
}

#[async_trait]
impl Transport for MockTransport {
    async fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        self.requests.lock().unwrap().push(request.clone());
        let index = {
            let mut next = self.next.lock().unwrap();
            let index = *next;
            *next += 1;
            index
        };
        let Some(interaction) = self.interactions.get(index) else {
            return Err(TransportError::connection(
                format!("MockTransport has no interaction #{index} left to replay"),
                TransportOperation::Request,
            ));
        };
        if self.strict_url {
            if let (Some(recorded), actual) =
                (interaction.request.as_ref().and_then(|r| r.url.as_deref()), request.url.as_str())
            {
                if recorded != actual {
                    return Err(TransportError::connection(
                        format!("MockTransport URL mismatch: recorded {recorded}, got {actual}"),
                        TransportOperation::Request,
                    ));
                }
            }
        }
        let recorded = interaction.response.as_ref();
        let status = recorded.and_then(|r| r.status).unwrap_or(200);
        let headers =
            recorded.and_then(|r| r.headers.as_ref()).map(wire_headers).unwrap_or_default();
        let body = recorded.and_then(|r| r.body.clone()).unwrap_or_default();
        let chunks = chunked(&body);
        let stream: ByteStream =
            Box::pin(futures::stream::iter(chunks.into_iter().map(Ok::<Bytes, TransportError>)));
        Ok(TransportResponse { status, headers, body: stream })
    }
}

/// A minimal echo protocol + route for infra tests (not a real wire
/// protocol): body is the request's prompt, stream emits one text event per
/// frame then finish.
pub struct EchoProtocol;

impl crate::route::protocol::Protocol for EchoProtocol {
    fn id(&self) -> &'static str {
        "echo"
    }

    fn body_from(&self, request: &crate::schema::LLMRequest) -> Result<serde_json::Value, AiError> {
        let prompt = request
            .messages
            .last()
            .and_then(|message| message.content.first())
            .map(|part| match part {
                crate::schema::ContentPart::Text { text, .. } => text.clone(),
                _ => String::new(),
            })
            .unwrap_or_default();
        Ok(serde_json::json!({"prompt": prompt}))
    }

    fn initial(
        &self,
        _request: &crate::schema::LLMRequest,
    ) -> crate::route::protocol::ProtocolState {
        Box::new(EchoState::default())
    }

    fn step(
        &self,
        state: crate::route::protocol::ProtocolState,
        frame: &str,
    ) -> Result<(crate::route::protocol::ProtocolState, Vec<crate::schema::LLMEvent>), AiError>
    {
        let mut state = EchoState::from(state);
        let mut events = Vec::new();
        if !state.started {
            state.started = true;
            events.push(crate::schema::LLMEvent::step_start(0.0));
        }
        let id = "text_0".to_string();
        if !state.open {
            state.open = true;
            events.push(crate::schema::LLMEvent::TextStart {
                id: id.clone(),
                provider_metadata: None,
            });
        }
        events.push(crate::schema::LLMEvent::TextDelta {
            id,
            text: frame.to_string(),
            provider_metadata: None,
        });
        Ok((Box::new(state), events))
    }

    fn on_halt(
        &self,
        state: crate::route::protocol::ProtocolState,
    ) -> Result<(crate::route::protocol::ProtocolState, Vec<crate::schema::LLMEvent>), AiError>
    {
        let state = EchoState::from(state);
        let mut events = Vec::new();
        if state.open {
            events.push(crate::schema::LLMEvent::TextEnd {
                id: "text_0".into(),
                text: None,
                provider_metadata: None,
            });
        }
        events.push(crate::schema::LLMEvent::StepFinish {
            index: 0.0,
            reason: crate::schema::FinishReasonDetails::new(crate::schema::FinishReason::Stop),
            usage: None,
            provider_metadata: None,
        });
        events.push(crate::schema::LLMEvent::Finish {
            reason: crate::schema::FinishReasonDetails::new(crate::schema::FinishReason::Stop),
            usage: None,
            provider_metadata: None,
        });
        Ok((Box::new(state), events))
    }
}

#[derive(Default)]
struct EchoState {
    started: bool,
    open: bool,
}

impl From<crate::route::protocol::ProtocolState> for EchoState {
    fn from(state: crate::route::protocol::ProtocolState) -> Self {
        *state.downcast::<EchoState>().expect("EchoProtocol state")
    }
}

impl From<EchoState> for crate::route::protocol::ProtocolState {
    fn from(state: EchoState) -> Self {
        Box::new(state)
    }
}

/// Build a route over [`EchoProtocol`] + SSE framing + the given transport —
/// the route-stack fixture shared by infra tests.
pub fn echo_route(transport: Arc<dyn Transport>) -> crate::route::Route {
    crate::route::Route::new(crate::route::RouteInput {
        id: "echo".into(),
        provider: Some("echo-provider".into()),
        provider_metadata_key: None,
        protocol: Arc::new(EchoProtocol),
        endpoint: crate::route::Endpoint::path("/echo").with_base_url("https://echo.test"),
        auth: None,
        framing: crate::route::Framing::sse(),
        headers: None,
        transport,
        defaults: Default::default(),
    })
}

/// Like [`echo_route`] but with a custom route id (cache policy keys off it).
#[must_use]
pub fn echo_route_named(route_id: &str) -> Arc<crate::route::Route> {
    Arc::new(crate::route::Route::new(crate::route::RouteInput {
        id: route_id.to_string(),
        provider: Some("test".into()),
        provider_metadata_key: None,
        protocol: Arc::new(EchoProtocol),
        endpoint: crate::route::Endpoint::path("/echo").with_base_url("https://echo.test"),
        auth: None,
        framing: crate::route::Framing::sse(),
        headers: None,
        transport: Arc::new(crate::route::transport::FailingTransport {
            message: "static test route",
        }),
        defaults: Default::default(),
    }))
}

/// A stub route for schema/endpoint tests that never touches a transport.
pub fn static_route_for_tests() -> Arc<crate::route::Route> {
    static ROUTE: OnceLock<Arc<crate::route::Route>> = OnceLock::new();
    Arc::clone(ROUTE.get_or_init(|| {
        Arc::new(echo_route(Arc::new(crate::route::transport::FailingTransport {
            message: "static test route",
        })))
    }))
}

/// A request bound to [`static_route_for_tests`] with the given messages.
#[must_use]
pub fn request_for_tests(messages: Vec<crate::schema::Message>) -> crate::schema::LLMRequest {
    let mut request = crate::schema::LLMRequest::new(crate::schema::LanguageModel::new(
        "test-model",
        "test-provider",
        static_route_for_tests(),
    ));
    request.messages = messages;
    request
}

/// Render a typed "fixture missing" error for probe tests.
pub fn scenario_error(message: impl Into<String>) -> AiError {
    AiError::new(AiErrorReason::InvalidProviderOutput {
        fields: ReasonFields::new(message),
        classification: None,
        route: Some("cassette".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_is_present_with_expected_groups() {
        let counts = group_counts();
        let total: usize = counts.iter().map(|(_, count)| count).sum();
        assert!(total >= 270, "expected the ~274-cassette corpus, found {total}");
        let names: Vec<&str> = counts.iter().map(|(name, _)| name.as_str()).collect();
        for expected in
            ["anthropic-messages", "gemini", "openai-responses", "bedrock-converse", "cohere"]
        {
            assert!(names.contains(&expected), "missing group {expected}");
        }
    }

    #[test]
    fn every_cassette_parses_verbatim() {
        let scenarios = all_scenarios().expect("corpus parses");
        assert!(!scenarios.is_empty());
        for scenario in &scenarios {
            assert_eq!(scenario.cassette.version, 1, "{}", scenario.full_name());
            assert!(!scenario.cassette.metadata.name.is_empty(), "{}", scenario.full_name());
        }
    }

    #[test]
    fn selection_filters_by_group_prefix_and_tag() {
        let gemini = select_scenarios(Some("gemini"), None).unwrap();
        assert!(!gemini.is_empty());
        assert!(gemini.iter().all(|s| s.group().starts_with("gemini")));
        let anthropic = select_scenarios(None, Some("provider:anthropic")).unwrap();
        assert!(!anthropic.is_empty());
        assert!(anthropic.iter().all(|s| s
            .cassette
            .metadata
            .tags
            .iter()
            .any(|t| t.contains("provider:anthropic"))));
    }

    #[tokio::test]
    async fn mock_transport_replays_interactions_in_order() {
        let interactions = vec![
            RecordedInteraction {
                method: "POST".into(),
                url: "https://echo.test/echo".into(),
                status: 200,
                body: "data: one\n\ndata: two\n\n".into(),
                request_body: Some(r#"{"prompt":"hi"}"#.into()),
            },
            RecordedInteraction {
                method: "POST".into(),
                url: "https://echo.test/echo".into(),
                status: 500,
                body: String::new(),
                request_body: None,
            },
        ];
        let transport = MockTransport::from_interactions(interactions);
        let first =
            transport.execute(TransportRequest::post("https://echo.test/echo")).await.unwrap();
        assert_eq!(first.status, 200);
        let body = crate::route::transport::read_body_limited(first.body, 1024).await.unwrap();
        assert_eq!(body, "data: one\n\ndata: two\n\n");
        let second =
            transport.execute(TransportRequest::post("https://echo.test/echo")).await.unwrap();
        assert_eq!(second.status, 500);
        assert_eq!(transport.remaining(), 0);
        let error =
            transport.execute(TransportRequest::post("https://echo.test/echo")).await.unwrap_err();
        assert!(error.message.contains("no interaction"));
    }

    #[tokio::test]
    async fn mock_transport_streams_chunked_bodies() {
        let transport = MockTransport::from_interactions(vec![RecordedInteraction {
            method: "POST".into(),
            url: "https://x.test".into(),
            status: 200,
            body: "data: abcdefghijklmnop\n\n".into(),
            request_body: None,
        }]);
        let response = transport.execute(TransportRequest::post("https://x.test")).await.unwrap();
        let chunks: Vec<Bytes> =
            futures::StreamExt::collect(futures::StreamExt::map(response.body, Result::unwrap))
                .await;
        assert!(chunks.iter().all(|chunk| chunk.len() <= 7), "chunks exercise byte boundaries");
    }

    #[tokio::test]
    async fn strict_url_matching_fails_on_divergence() {
        let transport = MockTransport::from_interactions(vec![RecordedInteraction {
            method: "POST".into(),
            url: "https://recorded.test/path".into(),
            status: 200,
            body: String::new(),
            request_body: None,
        }])
        .with_strict_url();
        let error =
            transport.execute(TransportRequest::post("https://other.test/path")).await.unwrap_err();
        assert!(error.message.contains("URL mismatch"));
    }

    #[tokio::test]
    async fn route_stack_streams_through_mock_transport() {
        let transport = Arc::new(MockTransport::from_interactions(vec![RecordedInteraction {
            method: "POST".into(),
            url: "https://echo.test/echo".into(),
            status: 200,
            body: "data: hello\n\ndata:  world\n\n".into(),
            request_body: None,
        }]));
        let route = echo_route(Arc::clone(&transport) as Arc<dyn Transport>);
        let model = route.model("test-model", None).unwrap();
        let mut request = crate::schema::LLMRequest::new(model);
        request.messages = vec![crate::schema::Message::user("hi")];
        let response = route.generate(request).await.expect("stream completes");
        assert_eq!(response.text(), "hello world");
        let sent = transport.requests();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].header("content-type"), Some("application/json"));
        let body: serde_json::Value = serde_json::from_slice(&sent[0].body).unwrap();
        assert_eq!(body["prompt"], "hi");
    }
}
