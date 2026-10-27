# Schema map: `@opencode/schema` (reference TS) → `oc-schema` (Rust)

Reference (v2.0.24): `packages/schema/src/*.ts` in the reference checkout
(98 non-test `.ts` files). Rust port: `crates/oc-schema/src/` (31 top-level
modules + the `config/` submodule with 18 config files). The mapping below
was derived from the provenance `//!` headers in each Rust module
(anchors of the form `` `file.ts` ``) and cross-checked by symbol search;
`?` marks an uncertain or partial mapping.

## Top-level modules

| Ref TS file | Rust module | Notes |
|---|---|---|
| `agent.ts` | `oc-schema/src/agent.rs` | |
| `command.ts` (Event) | `oc-schema/src/misc.rs` | event names in `misc_events.rs` registry data |
| `config.ts` | `oc-schema/src/config/mod.rs` | root config document |
| `connection.ts` | `oc-schema/src/integration.rs` | auth plumbing shared with `credential.ts` |
| `credential.ts` | `oc-schema/src/integration.rs` (+ ids in `id.rs`) | |
| `durable-event-manifest.ts` | `oc-core/src/bus/registry.rs` | mirror of the manifest; `oc-schema/src/lib.rs` TODO |
| `event-log.ts` | `oc-schema/src/events.rs` | `EventLog.Synced` replay boundary marker |
| `event-manifest.ts` | `oc-core/src/bus/registry.rs` | manifest index not ported to oc-schema (lib.rs TODO) |
| `event.ts` | `oc-schema/src/event.rs` | envelopes (`DurableEnvelope`/`EphemeralEnvelope`); ids in `id.rs` |
| `file-diff.ts` | `oc-schema/src/session_support.rs` | |
| `filesystem.ts` | `oc-schema/src/filesystem.rs` | event + entry/match types |
| `filesystem-v1.ts` | — (`?` oc-core/src/database, v1 migration) | v1 legacy; see lib.rs TODO |
| `form.ts` | `oc-schema/src/form.rs` | |
| `identifier.ts` | `oc-schema/src/identifier.rs` | branded-id machinery |
| `index.ts` | `oc-schema/src/lib.rs` | barrel file |
| `installation-event.ts` | `oc-schema/src/events.rs` | |
| `instruction-entry.ts` | `oc-schema/src/session_support.rs` | |
| `instruction.ts` | `oc-schema/src/session_support.rs` | |
| `integration-id.ts` | `oc-schema/src/id.rs` (`IntegrationId`, `IntegrationMethodId`) | |
| `integration.ts` | `oc-schema/src/integration.rs` | |
| `legacy-event.ts` | — (`?` oc-core/src/bus/registry.rs) | legacy event names; lib.rs TODO |
| `llm.ts` (schema pkg) | `oc-schema/src/common.rs` (`FinishReason`); core types in **oc-core/oc-ai** | see note row below |
| `location-event.ts` | `oc-schema/src/events.rs` | |
| `location.ts` | `oc-schema/src/location.rs` | |
| `lsp-event.ts` | `oc-schema/src/events.rs` | |
| `mcp-event.ts` | `oc-schema/src/events.rs` | |
| `mcp.ts` | `oc-schema/src/mcp.rs` | config subset in `config/mcp.rs` |
| `models-dev.ts` (schema pkg, Event) | **oc-core** (`provider_catalog/`) | see note row below |
| `model.ts` | `oc-schema/src/model.rs` | ids in `id.rs` |
| `money.ts` | `oc-schema/src/common.rs` | |
| `permission-saved.ts` | `oc-schema/src/permission.rs` (+ id in `id.rs`) | |
| `permission.ts` | `oc-schema/src/permission.rs` | |
| `permission-v1.ts` | — (`?` oc-core/src/database, v1 migration) | v1 legacy |
| `persistent-pty.ts` | `oc-schema/src/persistent_pty.rs` | |
| `plugin.ts` | `oc-schema/src/misc.rs` | |
| `project-id.ts` | `oc-schema/src/id.rs` (`ProjectId`) | |
| `project.ts` | `oc-schema/src/location.rs` | |
| `prompt-input.ts` | `oc-schema/src/prompt.rs` | wire form only (`FileAttachment` uri form) |
| `prompt.ts` | `oc-schema/src/prompt.rs` | |
| `provider.ts` | `oc-schema/src/provider.rs` | events in `events.rs`; ids in `id.rs` |
| `pty-ticket.ts` | — (`?` oc-server handlers, connect-ticket) | lib.rs TODO |
| `pty.ts` | `oc-schema/src/pty.rs` | |
| `question.ts` | `oc-schema/src/misc.rs` | |
| `reference.ts` | `oc-schema/src/reference.rs` | |
| `rpc.ts` | **oc-core** (`hooks.rs` RpcDefinition/RpcPort, `engine/`) | see note row below |
| `schema.ts` | `oc-schema/src/common.rs` | `RelativePath`/`AbsolutePath` brands, numeric brands |
| `server-event.ts` | `oc-schema/src/events.rs` | |
| `session-compaction-event.ts` | `oc-schema/src/events.rs` | |
| `session-error.ts` | `oc-schema/src/common.rs` (`SessionError`) | |
| `session-event.ts` | `oc-schema/src/session_event.rs` | all 51 session event definitions |
| `session-fork.ts` | `oc-schema/src/session_support.rs` | |
| `session-id.ts` | `oc-schema/src/id.rs` (`SessionId`) | |
| `session-inbox.ts` | `oc-schema/src/session_inbox.rs` | |
| `session-message.ts` | `oc-schema/src/session_message.rs` (+ ids in `id.rs`) | |
| `session-metadata.ts` | `oc-schema/src/session_support.rs` (`SessionMetadata`) | |
| `session-provider-context.ts` | `oc-schema/src/session_support.rs` | |
| `session-revert.ts` | `oc-schema/src/session_support.rs` | |
| `session-stats.ts` | `oc-schema/src/session_stats.rs` | |
| `session-status-event.ts` | `oc-schema/src/events.rs` | |
| `session-transfer.ts` | `oc-schema/src/session.rs` (`TransferData`) | |
| `session.ts` | `oc-schema/src/session.rs` | |
| `session-v1.ts` | — (`?` oc-core/src/database/v1_migration.rs) | v1 legacy |
| `shell.ts` | `oc-schema/src/shell.rs` | |
| `skill.ts` | `oc-schema/src/skill.rs` | |
| `snapshot.ts` | `oc-schema/src/id.rs` (`SnapshotId`) | store types: lib.rs TODO |
| `token-usage.ts` | `oc-schema/src/common.rs` (`TokenUsage`) | |
| `tool.ts` | `oc-schema/src/tool.rs` (`CallId` in `id.rs`) | |
| `tui-event.ts` | — (`?` oc-core/src/bus/registry.rs) | TUI event names, not wire schemas |
| `v1/filesystem.ts`, `v1/legacy-event.ts`, `v1/permission.ts`, `v1/session.ts` | — (`?` oc-core/src/database/v1_migration.rs) | v1 legacy schemas |
| `vcs-event.ts` | `oc-schema/src/events.rs` | |
| `vcs.ts` | `oc-schema/src/misc.rs` | |
| `websearch.ts` | `oc-schema/src/misc.rs` | |
| `workspace-event.ts` | `oc-schema/src/events.rs` | |
| `workspace-id.ts` | `oc-schema/src/id.rs` (`WorkspaceId`) | |
| `workspace.ts` | `oc-schema/src/misc.rs` | |
| `worktree-event.ts` | `oc-schema/src/events.rs` | |
| `worktree.ts` | `oc-schema/src/misc.rs` | |

Cross-module aggregators in oc-schema:

- `id.rs` — branded identifiers collected from many ref files
  (`session-id.ts`, `workspace-id.ts`, `project-id.ts`,
  `integration-id.ts`, `snapshot.ts`, and the `ID` types inside
  `event.ts`, `shell.ts`, `form.ts`, `permission.ts`,
  `permission-saved.ts`, `credential.ts`, `integration.ts`, `tool.ts`,
  `agent.ts`, `model.ts`, `provider.ts`, `session-message.ts`).
- `misc_events.rs` — the event-name registry data covering the small
  `*-event.ts` modules and namespace events from `agent.ts`, `model.ts`,
  `provider.ts`, `skill.ts`, `command.ts`, `config.ts`,
  `integration.ts`, `credential.ts`, `permission.ts`, `shell.ts`,
  `plugin.ts`, `project.ts`, `worktree.ts`, `websearch.ts`.
- `service.rs` — not from `@opencode/schema` at all: ports
  `packages/cli/src/services/service-config.ts` +
  `packages/client/src/effect/service.ts`.

## config/ submodule (1:1)

| Ref TS file | Rust module |
|---|---|
| `config/agent.ts` | `config/agent.rs` |
| `config/command.ts` | `config/command.rs` |
| `config/compaction.ts` | `config/compaction.rs` |
| `config/experimental.ts` | `config/experimental.rs` |
| `config/formatter.ts` | `config/formatter.rs` |
| `config/lsp.ts` | `config/lsp.rs` |
| `config/mcp.ts` | `config/mcp.rs` |
| `config/media.ts` | `config/media.rs` |
| `config/model.ts` | `config/model.rs` |
| `config/plugin.ts` | `config/plugin.rs` |
| `config/policy.ts` | `config/policy.rs` |
| `config/provider.ts` | `config/provider.rs` |
| `config/reference.ts` | `config/reference.rs` |
| `config/shell.ts` | `?` (folded into `config/mod.rs`; no dedicated file) |
| `config/tool-output.ts` | `config/tool_output.rs` |
| `config/warming.ts` | `config/warming.rs` |
| `config/watcher.ts` | `config/watcher.rs` |
| `config/websearch.ts` | `config/websearch.rs` |
| `config/worktree.ts` | `config/worktree.rs` |

## Note rows: schema families that live outside oc-schema

| Ref family | Rust location | Notes |
|---|---|---|
| LLM core types (`packages/ai/src/llm.ts`, `packages/ai/src/schema/*.ts`) | `crates/oc-ai` (+ `oc-core/src/session/runner/`, state machine annotated to `llm.ts`) | only `FinishReason` from schema-pkg `llm.ts` is in `oc-schema/src/common.rs` |
| Rpc (`packages/schema/src/rpc.ts`, `packages/core/src/rpc.ts`, plugin rpc plumbing) | `crates/oc-core/src/hooks.rs` (`RpcDefinition`/`RpcError`/`RpcPort`), `crates/oc-plugin-host/src/registrations.rs` | lib.rs TODO: rpc.ts not ported to oc-schema |
| ModelsDev (`packages/core/src/models-dev.ts`, `packages/core/src/plugin/models-dev.ts`) | `crates/oc-core/src/provider_catalog/` (`catalog.rs`, `refresh.rs`, `overlay.rs`, …) + `models-dev-snapshot.txt` | schema-pkg `models-dev.ts` event → `misc_events.rs` registry |

Open TODOs recorded in `oc-schema/src/lib.rs`: v1/ legacy schemas,
models-dev refresh plumbing, `rpc.ts`, `pty-ticket.ts`, `filesystem(.v1).ts`
non-event schemas, snapshot store types, `legacy-event.ts`,
`durable-event-manifest.ts`, `event-manifest.ts` index, and the
`Worktree.adopt` projection helper.
