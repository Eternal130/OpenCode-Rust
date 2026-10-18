//! Route layer: composition, framing, auth, endpoint, transport seam, and
//! the executor's failure classification.

pub mod auth;
pub mod client;
pub mod endpoint;
pub mod executor;
pub mod framing;
pub mod protocol;
pub mod transport;

pub use auth::{config, optional, value, AuthError, Definition as AuthDefinition, Headers, Secret};
pub use client::{merge_route_defaults, Route, RouteDefaults, RouteInput, RoutePatch};
pub use endpoint::{render, Endpoint, EndpointInput, EndpointPart, EndpointPatch};
pub use framing::{Framing, FramingDecoder, FramingKind};
pub use protocol::{Protocol, ProtocolState};
pub use transport::timeouts::TimeoutBudget;
pub use transport::{
    shared, ByteStream, FailingTransport, SharedTransport, Transport, TransportError,
    TransportOperation, TransportRequest, TransportResponse,
};

#[cfg(feature = "reqwest-transport")]
pub use transport::reqwest_transport::ReqwestTransport;
