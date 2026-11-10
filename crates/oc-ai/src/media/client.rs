//! Thin media client facade (port of `media-client.ts`, G-347B).
//!
//! The ref exposes each modality through a per-modality client whose
//! `generate` / `stream` / `start` / `resume` dispatch on the route's
//! `kind` (media-client.ts:13-62): queued routes start (+ resume) a
//! pollable generation; stream routes run the frame machine; inline
//! routes post one request. `start`/`resume` on a non-queued route fail
//! `UnsupportedOperation` — "`{provider}/{id}` is not a queued route; use
//! generate or stream".
//!
//! The Rust port stays dependency-light over the existing route lanes
//! ([`crate::media::route`]): each lane kind is boxed behind a small
//! object-safe trait so one [`MediaClient`] can dispatch like the ref,
//! without an executor context (routes already carry their transport).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;

use crate::media::model::{GenerationEvent, PollOptions};
use crate::media::protocol::{InlineProtocol, QueuedProtocol, StreamProtocol};
use crate::media::route::{Generation, InlineMediaRoute, QueuedMediaRoute, StreamMediaRoute};
use crate::schema::{AiError, AiErrorReason, ProviderId};

/// Which lane a route runs (media-client.ts dispatches `route.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteKind {
    Inline,
    Queued,
    Stream,
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;

/// One event a facade stream yields: queued-route observations (progress
/// polls), or the modality's own events (stream routes, and the response
/// events a queued/inline response expands into — ref `resultEvents`).
#[derive(Debug, Clone, PartialEq)]
pub enum ClientEvent<E> {
    /// A queued-route poll observation (ref filters `generation-finished`
    /// out and appends the expanded response at the end).
    Observation(GenerationEvent),
    /// A modality event — either a stream route's protocol event or one of
    /// the `responseEvents(response)` the modality expands a finished
    /// response into.
    Modality(E),
}

// ---------------------------------------------------------------------------
// Lane seams (object-safe over the three generic route kinds)
// ---------------------------------------------------------------------------

/// The inline lane seam.
pub trait InlineLane<Req, Resp>: Send + Sync {
    fn id(&self) -> &'static str;
    fn provider(&self) -> &'static str;
    fn generate<'a>(&'a self, request: &'a Req) -> BoxFuture<'a, Result<Resp, AiError>>;
}

/// The queued lane seam.
pub trait QueuedLane<Req, Resp>: Send + Sync {
    fn id(&self) -> &'static str;
    fn provider(&self) -> &'static str;
    fn start<'a>(
        &'a self,
        request: &'a Req,
    ) -> BoxFuture<'a, Result<GenerationHandle<Resp>, AiError>>;
    fn resume<'a>(
        &'a self,
        token: &'a serde_json::Value,
    ) -> BoxFuture<'a, Result<GenerationHandle<Resp>, AiError>>;
}

/// The stream lane seam: `generate` collects the stream; `events` runs it
/// in stream mode. The modality maps the collected
/// [`crate::media::model::CollectedResponse`] into its response type (the
/// ref's per-modality `collect` + service composition).
pub trait StreamLane<Req, E, Resp>: Send + Sync {
    fn id(&self) -> &'static str;
    fn provider(&self) -> &'static str;
    fn generate(&self, request: Req) -> BoxFuture<'static, Result<Resp, AiError>>;
    fn events(&self, request: Req) -> BoxStream<'_, Result<E, AiError>>;
}

impl<P> InlineLane<P::Request, P::Response> for InlineMediaRoute<P>
where
    P: InlineProtocol,
    P::Request: 'static,
    P::Response: Send,
{
    fn id(&self) -> &'static str {
        self.identity().id
    }
    fn provider(&self) -> &'static str {
        self.identity().provider
    }
    fn generate<'a>(
        &'a self,
        request: &'a P::Request,
    ) -> BoxFuture<'a, Result<P::Response, AiError>> {
        Box::pin(InlineMediaRoute::generate(self, request))
    }
}

/// The operations a [`GenerationHandle`] exposes over an erased
/// [`Generation`].
pub trait GenerationOps<Resp>: Send + Sync {
    fn token(&self) -> serde_json::Value;
    fn await_result(&self, options: PollOptions) -> BoxFuture<'_, Result<Resp, AiError>>;
    fn events(&self, options: PollOptions) -> BoxStream<'_, Result<GenerationEvent, AiError>>;
    fn cancel(&self) -> BoxFuture<'_, Result<(), AiError>>;
}

impl<P> GenerationOps<P::Response> for Generation<P>
where
    P: QueuedProtocol,
    P::Response: Send,
{
    fn token(&self) -> serde_json::Value {
        Generation::token(self).clone()
    }
    fn await_result(&self, options: PollOptions) -> BoxFuture<'_, Result<P::Response, AiError>> {
        Box::pin(Generation::await_result(self, options))
    }
    fn events(&self, options: PollOptions) -> BoxStream<'_, Result<GenerationEvent, AiError>> {
        Generation::events(self, options)
    }
    fn cancel(&self) -> BoxFuture<'_, Result<(), AiError>> {
        Box::pin(Generation::cancel(self))
    }
}

/// A pollable generation handed back by [`MediaClient::start`] /
/// [`MediaClient::resume`] (the ref `Generation<Response>`).
pub struct GenerationHandle<Resp: 'static> {
    inner: Box<dyn GenerationOps<Resp>>,
}

impl<Resp> GenerationHandle<Resp> {
    /// The serializable token — persist and [`MediaClient::resume`] it.
    pub fn token(&self) -> serde_json::Value {
        self.inner.token()
    }
    /// Poll to a terminal status, then read the result.
    pub async fn await_result(&self, options: PollOptions) -> Result<Resp, AiError> {
        self.inner.await_result(options).await
    }
    /// Observations until (and excluding) the finished event.
    pub fn events(&self, options: PollOptions) -> BoxStream<'_, Result<GenerationEvent, AiError>> {
        self.inner.events(options)
    }
    /// Cancel the generation (fails `UnsupportedOperation` where the
    /// protocol has no cancel endpoint).
    pub async fn cancel(&self) -> Result<(), AiError> {
        self.inner.cancel().await
    }
}

impl<P> QueuedLane<P::Request, P::Response> for QueuedMediaRoute<P>
where
    P: QueuedProtocol + 'static,
    P::Request: 'static,
    P::Response: Send,
{
    fn id(&self) -> &'static str {
        self.identity().id
    }
    fn provider(&self) -> &'static str {
        self.identity().provider
    }
    fn start<'a>(
        &'a self,
        request: &'a P::Request,
    ) -> BoxFuture<'a, Result<GenerationHandle<P::Response>, AiError>> {
        Box::pin(async move {
            let generation = QueuedMediaRoute::start(self, request).await?;
            Ok(GenerationHandle { inner: Box::new(generation) })
        })
    }
    fn resume<'a>(
        &'a self,
        token: &'a serde_json::Value,
    ) -> BoxFuture<'a, Result<GenerationHandle<P::Response>, AiError>> {
        Box::pin(async move {
            let generation = QueuedMediaRoute::resume(self, token).await?;
            Ok(GenerationHandle { inner: Box::new(generation) })
        })
    }
}

/// A stream route adapter: the modality's `collect` fn plus the mapping
/// from the collected [`crate::media::model::CollectedResponse`] into the
/// modality's response type.
pub struct StreamLaneAdapter<P, Resp>
where
    P: StreamProtocol + 'static,
{
    pub route: Arc<StreamMediaRoute<P>>,
    pub extract_response: fn(&crate::media::model::CollectedResponse) -> Result<Resp, AiError>,
}

impl<P, Resp> StreamLane<P::Request, P::Event, Resp> for StreamLaneAdapter<P, Resp>
where
    P: StreamProtocol + 'static,
    P::Request: 'static,
    P::Event: Send + 'static,
    Resp: 'static,
{
    fn id(&self) -> &'static str {
        self.route.identity().id
    }
    fn provider(&self) -> &'static str {
        self.route.identity().provider
    }
    fn generate(&self, request: P::Request) -> BoxFuture<'static, Result<Resp, AiError>> {
        let route = Arc::clone(&self.route);
        let extract = self.extract_response;
        Box::pin(async move {
            let collected = route.generate(request).await?;
            extract(&collected)
        })
    }
    fn events(&self, request: P::Request) -> BoxStream<'_, Result<P::Event, AiError>> {
        // The stream lane's protocol events arrive in stream mode.
        let route = Arc::clone(&self.route);
        Box::pin(async_stream::stream! {
            let mut stream = match route.stream(request, crate::media::model::Mode::Stream).await {
                Ok(stream) => stream,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };
            while let Some(item) = futures::StreamExt::next(&mut stream).await {
                yield item;
            }
        })
    }
}

enum Lane<Req: 'static, E: 'static, Resp: 'static> {
    Inline(Box<dyn InlineLane<Req, Resp>>),
    Queued(Box<dyn QueuedLane<Req, Resp>>),
    Stream(Box<dyn StreamLane<Req, E, Resp>>),
}

// ---------------------------------------------------------------------------
// The client (media-client.ts:13-62)
// ---------------------------------------------------------------------------

/// The per-modality media client (ref `media-client.ts` `Interface` +
/// `make`): one modality name, one response→events expander, one route.
/// `Req: Clone` because the stream lane takes its request by value.
pub struct MediaClient<Req: Clone + 'static, E: Send + 'static, Resp: Send + 'static> {
    modality: &'static str,
    /// A completed response expanded into the streaming event shape
    /// (`responseEvents`, media-client.ts:23).
    response_events: fn(&Resp) -> Vec<E>,
    lane: Lane<Req, E, Resp>,
}

impl<Req: Clone, E: Send, Resp: Send> MediaClient<Req, E, Resp> {
    #[must_use]
    pub fn inline(
        modality: &'static str,
        lane: impl InlineLane<Req, Resp> + 'static,
        response_events: fn(&Resp) -> Vec<E>,
    ) -> Self {
        Self { modality, response_events, lane: Lane::Inline(Box::new(lane)) }
    }

    #[must_use]
    pub fn queued(
        modality: &'static str,
        lane: impl QueuedLane<Req, Resp> + 'static,
        response_events: fn(&Resp) -> Vec<E>,
    ) -> Self {
        Self { modality, response_events, lane: Lane::Queued(Box::new(lane)) }
    }

    #[must_use]
    pub fn stream_route(
        modality: &'static str,
        lane: impl StreamLane<Req, E, Resp> + 'static,
        response_events: fn(&Resp) -> Vec<E>,
    ) -> Self {
        Self { modality, response_events, lane: Lane::Stream(Box::new(lane)) }
    }

    /// The route kind this client dispatches on.
    #[must_use]
    pub fn kind(&self) -> RouteKind {
        match &self.lane {
            Lane::Inline(_) => RouteKind::Inline,
            Lane::Queued(_) => RouteKind::Queued,
            Lane::Stream(_) => RouteKind::Stream,
        }
    }

    fn route_id(&self) -> (&'static str, &'static str) {
        match &self.lane {
            Lane::Inline(lane) => (lane.provider(), lane.id()),
            Lane::Queued(lane) => (lane.provider(), lane.id()),
            Lane::Stream(lane) => (lane.provider(), lane.id()),
        }
    }

    /// `start`/`resume` on a non-queued route
    /// (media-client.ts:36-41): `UnsupportedOperation` naming the route.
    fn not_queued(&self, operation: &str) -> AiError {
        let (provider, id) = self.route_id();
        AiError::new(AiErrorReason::UnsupportedOperation {
            fields: crate::schema::ReasonFields::new(format!(
                "{provider}/{id} is not a queued route; use generate or stream"
            )),
            operation: format!("{}.{}", self.modality, operation),
            provider: Some(ProviderId::new(provider)),
            route: Some(id.to_string()),
        })
    }

    /// Submit a queued generation (media-client.ts:42-45). Only queued
    /// routes; inline and stream routes fail [`Self::not_queued`].
    pub async fn start(&self, request: &Req) -> Result<GenerationHandle<Resp>, AiError> {
        match &self.lane {
            Lane::Queued(lane) => lane.start(request).await,
            _ => Err(self.not_queued("start")),
        }
    }

    /// Rebuild a generation from a persisted token (media-client.ts:46-49).
    pub async fn resume(
        &self,
        token: &serde_json::Value,
    ) -> Result<GenerationHandle<Resp>, AiError> {
        match &self.lane {
            Lane::Queued(lane) => lane.resume(token).await,
            _ => Err(self.not_queued("resume")),
        }
    }

    /// Generate a completed response (media-client.ts:50-53): non-queued
    /// routes call the route directly; queued routes start and await with
    /// the default poll options.
    pub async fn generate(&self, request: &Req) -> Result<Resp, AiError> {
        match &self.lane {
            Lane::Inline(lane) => lane.generate(request).await,
            Lane::Stream(lane) => lane.generate(request.clone()).await,
            Lane::Queued(lane) => {
                let generation = lane.start(request).await?;
                generation.await_result(PollOptions::default()).await
            }
        }
    }

    /// The event stream (media-client.ts:54-61): stream routes run the
    /// frame machine; queued routes stream observations then the expanded
    /// response (`resultEvents`); inline routes expand the one response.
    /// The returned stream borrows the client (`'s`) — like the ref, the
    /// stream runs against the route it was started from.
    pub async fn stream(
        &self,
        request: Req,
    ) -> Result<BoxStream<'_, Result<ClientEvent<E>, AiError>>, AiError> {
        let response_events = self.response_events;
        match &self.lane {
            Lane::Stream(lane) => {
                let events = lane.events(request);
                Ok(Box::pin(async_stream::stream! {
                    let mut events = events;
                    while let Some(item) = futures::StreamExt::next(&mut events).await {
                        match item {
                            Ok(event) => yield Ok(ClientEvent::Modality(event)),
                            Err(error) => yield Err(error),
                        }
                    }
                }))
            }
            Lane::Queued(lane) => {
                // start, then observations (minus the finished event) +
                // the expanded result (resultEvents, generation.ts:159-173).
                let generation = lane.start(&request).await?;
                Ok(Box::pin(async_stream::stream! {
                    let generation = generation;
                    let mut observations = generation.events(PollOptions::default());
                    while let Some(item) = futures::StreamExt::next(&mut observations).await {
                        match item {
                            Ok(GenerationEvent::Finished(_)) => {}
                            Ok(event) => yield Ok(ClientEvent::Observation(event)),
                            Err(error) => {
                                yield Err(error);
                                return;
                            }
                        }
                    }
                    match generation.await_result(PollOptions::default()).await {
                        Ok(response) => {
                            for event in response_events(&response) {
                                yield Ok(ClientEvent::Modality(event));
                            }
                        }
                        Err(error) => yield Err(error),
                    }
                }))
            }
            Lane::Inline(lane) => {
                let response = lane.generate(&request).await?;
                let events = response_events(&response);
                Ok(Box::pin(futures::stream::iter(
                    events.into_iter().map(|event| Ok(ClientEvent::Modality(event))),
                )))
            }
        }
    }
}
