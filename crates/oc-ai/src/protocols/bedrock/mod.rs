//! Amazon Bedrock LLM protocols (port of the bedrock slices of
//! `packages/ai/src/protocols` + `providers/amazon-bedrock*.ts`):
//!
//! - [`sigv4`]: AWS `SigV4` request signing + the bearer/`SigV4` auth
//!   resolution Bedrock routes use (`AWS_BEARER_TOKEN_BEDROCK` bearer wins
//!   over `SigV4`, matching the AWS SDK precedence),
//! - [`event_stream`]: the AWS event-stream binary framing
//!   (`vnd.amazon.eventstream`) Bedrock streams responses in,
//! - [`converse`]: the Converse protocol — request lowering and the
//!   streaming state machine — plus the route factories
//!   ([`converse::bedrock_converse_route`], the converse-stream endpoint)
//!   and [`converse::bedrock_invoke_route`] (anthropic-on-bedrock, the
//!   `/model/{id}/invoke-stream` shape over the Anthropic Messages
//!   protocol).

pub mod converse;
pub mod credentials;
pub mod event_stream;
pub mod sigv4;

pub use converse::{bedrock_converse_route, bedrock_invoke_route, BedrockConverseProtocol};
pub use sigv4::{resolve_auth, resolve_region, BedrockAuthMode, BedrockAuthOptions, Credentials};
