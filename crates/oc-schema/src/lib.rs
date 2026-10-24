// Pedantic clippy concessions for a wire-format crate: many pure getters and
// deliberate number casts (JS number parity in `Num`, identifier bit math).
#![allow(
    clippy::must_use_candidate,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

//! opencode shared schema types (serde + schemars) — Rust port of
//! `@opencode/schema` (v2.0.24).
//!
//! Wire-format ground truth: `packages/protocol/openapi.json` component
//! schemas. Tagged unions mirror v2's `Schema.toTaggedUnion` via serde's
//! internal tagging; the public event envelope is
//! `{id, created, metadata?, type, durable?, location?, data}` where `durable`
//! is `{aggregateID, seq, version}` and is required on durable events only.
//! Numbers use [`common::Num`] so whole values serialize as JSON integers,
//! matching JavaScript output.

pub mod agent;
pub mod common;
pub mod config;
pub mod event;
pub mod events;
pub mod filesystem;
pub mod form;
pub mod id;
pub mod identifier;
pub mod integration;
pub mod location;
pub mod mcp;
pub mod misc;
pub mod misc_events;
pub mod model;
pub mod permission;
pub mod persistent_pty;
pub mod prompt;
pub mod provider;
pub mod pty;
pub mod reference;
pub mod service;
pub mod session;
pub mod session_event;
pub mod session_inbox;
pub mod session_message;
pub mod session_stats;
pub mod session_support;
pub mod shell;
pub mod skill;
pub mod tool;

pub use agent::{Info as AgentInfo, Mode as AgentMode};
pub use common::{
    AbsolutePath, FinishReason, Num, RelativePath, SessionError, TokenUsage, TokenUsageCache, Usd,
    UsdPerMillionTokens,
};
pub use event::{DurableEnvelope, DurableInfo, EphemeralEnvelope};
pub use id::{
    AgentId, AgentName, CallId, CredentialId, EventId, FormId, IntegrationAttemptId, IntegrationId,
    IntegrationMethodId, MessageId, ModelId, ModelVariantId, PermissionId, PermissionSavedId,
    PluginId, ProjectId, ProviderId, SessionId, ShellId, SkillId, SkillName, SnapshotId,
    WebSearchId, WorkspaceId,
};
pub use location::{Info as LocationInfo, Ref as LocationRef};
pub use model::{Info as ModelInfo, Ref as ModelRef};
pub use permission::{
    Effect as PermissionEffect, Reply as PermissionReply, Request as PermissionRequest,
    Rule as PermissionRule, Ruleset as PermissionRuleset,
};
pub use provider::{Info as ProviderInfo, Request as ProviderRequest};
pub use session_message::{
    AssistantContent, Compaction as MessageCompaction, Info as MessageInfo, ToolState,
};
pub use session_support::{ProviderContext, Revert, SessionMetadata};

// TODO(port): v1/ legacy schemas, models-dev refresh plumbing, rpc.ts,
// pty-ticket.ts, filesystem(.v1).ts non-event schemas, snapshot store
// types, legacy-event.ts, durable-event-manifest.ts,
// event-manifest.ts index, Worktree.adopt projection helper.
