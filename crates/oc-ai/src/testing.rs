//! `TestLLM` equivalent (port of `src/testing.ts`) for downstream core
//! tests: queue event responses / serve from a responder / gate controls,
//! observed requests, and terminal-event enforcement identical to the real
//! client.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use futures::StreamExt as _;

use crate::schema::{
    AiError, FinishReason, FinishReasonDetails, LLMEvent, LLMRequest, LLMResponse, Usage,
};

/// One queued response: a list of events (streams and errors can be built by
/// pushing `provider-error` / queuing error responses via [`TestLlm::push_error`]).
#[derive(Clone)]
pub struct TestResponse {
    pub events: Vec<LLMEvent>,
    pub error: Option<AiError>,
}

impl TestResponse {
    #[must_use]
    pub fn events(events: Vec<LLMEvent>) -> Self {
        Self { events, error: None }
    }

    #[must_use]
    pub fn error(error: AiError) -> Self {
        Self { events: Vec::new(), error: Some(error) }
    }
}

/// Convenience builders (port of `TestLLM.complete/stop/toolCalls/text`).
#[must_use]
pub fn complete(reason: FinishReason, mut events: Vec<LLMEvent>) -> TestResponse {
    let mut all = vec![LLMEvent::step_start(0.0)];
    all.append(&mut events);
    all.push(LLMEvent::StepFinish {
        index: 0.0,
        reason: detail(reason),
        usage: None,
        provider_metadata: None,
    });
    all.push(LLMEvent::Finish { reason: detail(reason), usage: None, provider_metadata: None });
    TestResponse::events(all)
}

fn detail(reason: FinishReason) -> FinishReasonDetails {
    FinishReasonDetails::new(reason)
}

#[must_use]
pub fn stop(events: Vec<LLMEvent>) -> TestResponse {
    complete(FinishReason::Stop, events)
}

#[must_use]
pub fn tool_calls(events: Vec<LLMEvent>) -> TestResponse {
    complete(FinishReason::ToolCalls, events)
}

#[must_use]
pub fn text(value: &str) -> TestResponse {
    stop(vec![
        LLMEvent::TextStart { id: "text_1".into(), provider_metadata: None },
        LLMEvent::TextDelta { id: "text_1".into(), text: value.into(), provider_metadata: None },
        LLMEvent::TextEnd { id: "text_1".into(), text: None, provider_metadata: None },
    ])
}

#[must_use]
pub fn text_with_usage(value: &str, input_tokens: f64) -> TestResponse {
    let mut all = vec![
        LLMEvent::TextStart { id: "text_1".into(), provider_metadata: None },
        LLMEvent::TextDelta { id: "text_1".into(), text: value.into(), provider_metadata: None },
        LLMEvent::TextEnd { id: "text_1".into(), text: None, provider_metadata: None },
    ];
    let usage = Usage {
        input_tokens: Some(input_tokens),
        non_cached_input_tokens: Some(input_tokens),
        ..Default::default()
    };
    let mut response = complete(FinishReason::Stop, std::mem::take(&mut all));
    if let Some(LLMEvent::StepFinish { usage: slot, .. }) =
        response.events.iter_mut().rev().find(|e| matches!(e, LLMEvent::StepFinish { .. }))
    {
        *slot = Some(usage);
    }
    response
}

#[must_use]
pub fn tool(id: &str, name: &str, input: serde_json::Value) -> TestResponse {
    tool_calls(vec![LLMEvent::ToolCall {
        id: id.into(),
        name: name.into(),
        namespace: None,
        input,
        provider_executed: None,
        provider_metadata: None,
    }])
}

#[derive(Default)]
struct Inner {
    requests: Vec<LLMRequest>,
    responses: VecDeque<TestResponse>,
    fallback: Option<TestResponse>,
    responder: Option<Arc<dyn Fn(&LLMRequest) -> TestResponse + Send + Sync>>,
    /// Gate: when set, streams wait on the release latch after registering.
    gate: Option<Arc<tokio::sync::Notify>>,
    arrivals: Arc<tokio::sync::Notify>,
}

/// In-memory LLM client for tests (port of `TestLLM`). Clone-safe: all state
/// lives behind one shared mutex.
#[derive(Clone)]
pub struct TestLlm {
    inner: Arc<Mutex<Inner>>,
}

impl Default for TestLlm {
    fn default() -> Self {
        Self::new()
    }
}

impl TestLlm {
    #[must_use]
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(Inner::default())) }
    }

    /// Queue one-shot responses (port of `push`).
    pub fn push(&self, responses: impl IntoIterator<Item = TestResponse>) {
        self.inner.lock().unwrap().responses.extend(responses);
    }

    /// Replace the fallback without changing queued responses (port of
    /// `always`).
    pub fn always(&self, response: TestResponse) {
        self.inner.lock().unwrap().fallback = Some(response);
    }

    /// Answer requests after the one-shot queue is exhausted (port of
    /// `serve`); receives the original request.
    pub fn serve<F>(&self, responder: F)
    where
        F: Fn(&LLMRequest) -> TestResponse + Send + Sync + 'static,
    {
        self.inner.lock().unwrap().responder = Some(Arc::new(responder));
    }

    /// Snapshot of requests observed at execution time.
    #[must_use]
    pub fn requests(&self) -> Vec<LLMRequest> {
        self.inner.lock().unwrap().requests.clone()
    }

    /// Wait until `count` requests have arrived (not output completion).
    pub async fn wait(&self, count: usize) {
        loop {
            let notify = {
                let inner = self.inner.lock().unwrap();
                if inner.requests.len() >= count {
                    return;
                }
                inner.arrivals.clone()
            };
            notify.notified().await;
        }
    }

    /// Install a gate: subsequent streams hold after registering until
    /// [`GateHandle::release`] (port of `gate`). Replaces any previous gate.
    #[must_use]
    pub fn gate(&self) -> GateHandle {
        let notify = Arc::new(tokio::sync::Notify::new());
        let handle = GateHandle { notify: Arc::clone(&notify) };
        self.inner.lock().unwrap().gate = Some(notify);
        handle
    }

    fn take(&self, request: &LLMRequest) -> Result<TestResponse, AiError> {
        let mut inner = self.inner.lock().unwrap();
        inner.requests.push(request.clone());
        let arrivals = inner.arrivals.clone();
        let response = inner
            .responses
            .pop_front()
            .or_else(|| inner.responder.as_ref().map(|respond| respond(request)))
            .or_else(|| inner.fallback.clone());
        // Wake waiters after the reply is visible.
        arrivals.notify_waiters();
        let Some(response) = response else {
            let count = inner.requests.len();
            return Err(AiError::invalid_request(format!(
                "TestLLM has no response for request {count}"
            )));
        };
        Ok(response)
    }

    /// Stream events for one request with terminal-event enforcement
    /// (port of `TestInterface.stream`).
    pub fn stream(
        &self,
        request: LLMRequest,
    ) -> impl futures::Stream<Item = Result<LLMEvent, AiError>> + Send + 'static {
        let this = self.clone();
        async_stream::stream! {
            let response = match this.take(&request) {
                Ok(response) => response,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };
            // Hold on the gate after registering, if one is active.
            let gate = this.inner.lock().unwrap().gate.clone();
            if let Some(gate) = gate {
                gate.notified().await;
            }
            if let Some(error) = response.error {
                for event in response.events {
                    yield Ok(event);
                }
                yield Err(error);
                return;
            }
            let mut terminal = false;
            for event in response.events {
                if terminal {
                    yield Err(AiError::event_error("test-llm", format!("Provider emitted {} after the terminal event", event.kind())));
                    return;
                }
                if event.is_terminal() {
                    terminal = true;
                }
                yield Ok(event);
            }
            if !terminal {
                yield Err(AiError::new(AiErrorReason::InvalidProviderOutput {
                    fields: crate::schema::ReasonFields::new("TestLLM response ended without a terminal finish event"),
                    classification: Some(crate::schema::CLASSIFICATION_INCOMPLETE_STREAM.into()),
                    route: Some("test-llm".into()),
                }));
            }
        }
    }

    /// Collect a response (port of `TestInterface.generate`).
    pub async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, AiError> {
        let stream = self.stream(request);
        let state = stream
            .fold(Ok(crate::schema::ResponseState::empty()), |state, item| async move {
                match (state, item) {
                    (Ok(state), Ok(event)) => Ok(state.reduce(event)),
                    (Ok(_), Err(error)) => Err(error),
                    (Err(error), _) => Err(error),
                }
            })
            .await?;
        state.complete().ok_or_else(|| {
            AiError::new(AiErrorReason::InvalidProviderOutput {
                fields: crate::schema::ReasonFields::new(
                    "TestLLM response ended without a terminal finish event",
                ),
                classification: Some(crate::schema::CLASSIFICATION_INCOMPLETE_STREAM.into()),
                route: Some("test-llm".into()),
            })
        })
    }
}

use crate::schema::AiErrorReason;

/// Gate controls (port of `TestLLM.Gate`): `started` awaits the first held
/// stream; `release` opens the gate.
pub struct GateHandle {
    notify: Arc<tokio::sync::Notify>,
}

impl GateHandle {
    /// Wait until at least one stream is being held by this gate.
    pub async fn started(&self) {
        // Streams notify on registration; arrival is observable via the
        // queue, so a short yield is sufficient in single-task tests.
        tokio::task::yield_now().await;
    }

    /// Open the gate; held streams resume.
    pub fn release(&self) {
        self.notify.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> LLMRequest {
        crate::test_support::request_for_tests(vec![])
    }

    #[tokio::test]
    async fn queues_and_streams_responses() {
        let llm = TestLlm::new();
        llm.push([text("hello"), tool("call_1", "lookup", serde_json::json!({"q": 1}))]);
        let first = llm.generate(request()).await.unwrap();
        assert_eq!(first.text(), "hello");
        let second = llm.generate(request()).await.unwrap();
        assert_eq!(second.tool_calls().len(), 1);
        assert_eq!(llm.requests().len(), 2);
    }

    #[tokio::test]
    async fn always_answers_after_the_queue_is_exhausted() {
        let llm = TestLlm::new();
        llm.push([text("first")]);
        llm.always(text("fallback"));
        assert_eq!(llm.generate(request()).await.unwrap().text(), "first");
        assert_eq!(llm.generate(request()).await.unwrap().text(), "fallback");
    }

    #[tokio::test]
    async fn serve_sees_the_original_request() {
        let llm = TestLlm::new();
        llm.serve(|request| {
            let prompt = matches!(
                request.messages.first().and_then(|m| m.content.first()),
                Some(crate::schema::ContentPart::Text { text, .. }) if text == "ping"
            );
            text(if prompt { "pong" } else { "?" })
        });
        let request =
            crate::test_support::request_for_tests(vec![crate::schema::Message::user("ping")]);
        assert_eq!(llm.generate(request).await.unwrap().text(), "pong");
    }

    #[tokio::test]
    async fn missing_responses_fail_with_a_typed_error() {
        let llm = TestLlm::new();
        let error = llm.generate(request()).await.unwrap_err();
        assert!(error.message().contains("TestLLM has no response"));
    }

    #[tokio::test]
    async fn response_without_a_terminal_finish_fails() {
        let llm = TestLlm::new();
        llm.push([TestResponse::events(vec![LLMEvent::TextDelta {
            id: "t".into(),
            text: "partial".into(),
            provider_metadata: None,
        }])]);
        let error = llm.generate(request()).await.unwrap_err();
        assert!(matches!(error.reason, AiErrorReason::InvalidProviderOutput { .. }));
    }

    #[tokio::test]
    async fn queued_errors_fail_the_stream_after_events() {
        let llm = TestLlm::new();
        llm.push([TestResponse::error(AiError::authentication("no key"))]);
        let error = llm.generate(request()).await.unwrap_err();
        assert!(matches!(error.reason, AiErrorReason::Authentication { .. }));
    }

    #[tokio::test]
    async fn wait_unblocks_on_arrivals() {
        let llm = TestLlm::new();
        llm.always(text("ok"));
        let llm2 = llm.clone();
        let handle = tokio::spawn(async move {
            llm2.wait(1).await;
        });
        let _ = llm.generate(request()).await;
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn usage_flows_through_complete_events() {
        let llm = TestLlm::new();
        llm.push([text_with_usage("hi", 12.0)]);
        let response = llm.generate(request()).await.unwrap();
        let usage = response.usage.expect("usage present");
        assert_eq!(usage.input_tokens, Some(12.0));
        assert_eq!(usage.non_cached_input_tokens, Some(12.0));
    }

    #[test]
    fn complete_builders_emit_terminal_events() {
        let response = complete(FinishReason::Stop, vec![]);
        assert_eq!(
            response.events.iter().map(LLMEvent::kind).collect::<Vec<_>>(),
            ["step-start", "step-finish", "finish"]
        );
        let tool_response = tool("c1", "bash", serde_json::json!({"cmd": "ls"}));
        assert!(tool_response.events.iter().any(|e| e.kind() == "tool-call"));
    }
}
