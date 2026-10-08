//! The semantic API contract of one model server family (port of
//! `src/route/protocol.ts`).
//!
//! A [`Protocol`] owns "what does this API look like": how a common
//! [`LLMRequest`] becomes a provider-native JSON body, and how the framed
//! streaming response decodes back into common [`LLMEvent`]s via a pure
//! state machine (`initial` → `step`* → `terminal`/`on_halt`). It is **not**
//! a deployment: URL, headers and auth belong to the route
//! ([`crate::route::Route`]).
//!
//! TS's `Protocol<Body, Frame, Event, State>` generics are intentionally
//! erased here (the TS route layer erases them too — `AnyRoute`): `Body` is
//! the JSON body as `serde_json::Value`, `Frame` is the framed string, and
//! `State` is an opaque boxed value the protocol owns. Concrete protocol
//! implementations (later lanes: `protocols/anthropic_messages.rs` etc.)
//! implement this trait directly over their typed internals and downcast
//! inside `step`.

use std::any::Any;

use serde_json::Value;

use crate::schema::options::SanitizerCompatibility;
use crate::schema::{AiError, LLMEvent, LLMRequest};

/// Opaque per-response parser state owned by one protocol implementation.
pub type ProtocolState = Box<dyn Any + Send>;

/// The protocol contract (port of `Protocol<Body, Frame, Event, State>`).
pub trait Protocol: Send + Sync {
    /// Stable id for the wire protocol implementation (e.g.
    /// `"anthropic-messages"`).
    fn id(&self) -> &str;

    /// Request side: build the provider-native body from a common request
    /// (port of `body.from`). The route JSON-encodes the result.
    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError>;

    /// Initial parser state; called once per response with the resolved
    /// request (port of `stream.initial`).
    fn initial(&self, request: &LLMRequest) -> ProtocolState;

    /// Translate one decoded frame into emitted events plus the next state
    /// (port of `stream.step`). Pure: same state + frame ⇒ same output.
    fn step(&self, state: ProtocolState, frame: &str) -> Result<(ProtocolState, Vec<LLMEvent>), AiError>;

    /// Optional request-completion signal for transports that do not end
    /// naturally (port of `stream.terminal`); `false` by default.
    fn terminal(&self, _frame: &str) -> bool {
        false
    }

    /// Optional flush emitted when the framed stream ends (port of
    /// `stream.onHalt`); no events by default.
    fn on_halt(&self, state: ProtocolState) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        Ok((state, Vec::new()))
    }

    /// Whether `body_from` lowers `Message::effort` markers (port of
    /// `supportsEffortUpdates`); wrappers must forward it.
    fn supports_effort_updates(&self, _request: &LLMRequest) -> bool {
        false
    }

    /// Tool schema sanitizer for every model on this protocol unless the
    /// model's compatibility sets one (port of `sanitizer`).
    fn sanitizer(&self) -> Option<SanitizerCompatibility> {
        None
    }
}
