# Lane audit-E — v2 CORE SERVICES vs the Rust port (oc-core / oc-ai)

**Scope**: process-level semantics under the API/TUI — session lifecycle, jobs, subagents, plugins, providers/auth, tools, MCP, PTY, config, events, permissions, vcs/worktree, files/images — `/opt/opencode-ref/packages/core/src` (v2) vs `crates/oc-core` + `crates/oc-ai` (+ the oc-server handlers that are the only callers of many core services). Read-only audit. This report decides, for each TUI-audit dead surface, whether the fix lands in **core** or in **TUI wiring**.

**Method**: enumerated ref core's exported service interfaces and operations per domain (file:line), walked the rust equivalents, diffed operations; verified "route exists" vs "route is backed by real core code" (several routes are documented stubs — `TODO(phase-9)` markers in oc-server handlers).

---

## 0. Executive summary

The **durable execution core is real and close to parity**: inbox/execution/drain/compaction (incl. native Anthropic + OpenAI-Responses compaction), retry, titles, revert, jobs, subagents, permissions, forms, PTY daemon, MCP client/registry, prompt admission (files/agents/skills/steer/queue), event-sourced projector/store, config discovery/normalize/watch, 15/15 built-in tools. Real LLM traffic flows (`RouteLlmClient` over oc-ai routes w/ reqwest).

The gaps cluster in **everything that touches the outside world beyond plain LLM APIs**: the integration/auth host (all provider login flows), provider-specific protocol families, media generation, live catalog refresh, websearch wiring, worktree git-side, and a handful of **server handlers that stub or mis-decode core machinery that exists** (fork, config.update, websearch, command.list).

**Counts** (18 domains): **parity 8**, **partial 7**, **missing 3** (integration/auth host; media/image+speech+transcription+video protocols; enterprise/account+managed-policy). Operation-level: ~140 ref core operations inventoried; ~95 parity, ~28 partial, ~17 missing/divergent.

---

## 1. Domain tables

Legend: ✅ parity · 🟡 partial (ops listed) · ❌ missing · ⚠️ divergent semantics.

### 1.1 Session lifecycle (ref `session/session.ts:346-368` — 21 ops + store/transfer/move/title/stats/history/diff/generate)

| Ref operation (file:line) | Rust | Class |
|---|---|---|
| get/message/view/rename/setMetadata/setPermissions/switchAgent/switchModel (session.ts:52-108) | handlers/session.rs:444-541 over store/bus | ✅ |
| inbox list/cancel/update (delivery) (session.ts:126-144) | `session/inbox.rs` (834 L) + handlers 1072-1116 | ✅ |
| prompt (steer/queue, files/agents/skills/metadata, resume) (session.ts:145-178, prompt.ts:29-96) | handlers/session.rs:543-591 (`materialize_files`), `session/runner/context.rs` | ✅ |
| synthetic / compact / wait / resume / interrupt / skill (session.ts:246-318) | engine `compact`/`interrupt`; handlers 593-684, 1213 | ✅ |
| revert stage/clear/commit (session.ts:319-345, revert.ts) | `session/revert.rs` + handlers 1018-1070 | ✅ |
| create/list (store.ts: create/claim/list w/ anchors) | `session/store.rs` (700 L) | ✅ |
| fork (projector.ts:144-230: settled-copy, fork-title, instruction seeding) | core projector.rs:571-680 **complete**; **but** oc-server handler decodes `before` as a *string* (handlers/session.rs:748-770, ref schema `session-fork.ts` = `{type:"before"\|"through", messageID}`) and pre-creates the row so the projector's `INSERT OR IGNORE` hits `ALREADY_PROJECTED` → **forks copy nothing, title not fork-styled** | ⚠️ server bug over correct core |
| move (move.ts: Destination* errors, transport) | engine `move_session` admits `Move_` inbox item (engine/mod.rs:1068-1096) | 🟡 move item runs at drain boundary; ref's full destination validation/errors not surfaced; `session_move` opencode-tool is a declared stub (tool/opencode_tools.rs:89-116) |
| transfer export/import + sanitize (transfer.ts:33-50) | handlers/session.rs:1257-1377 | ✅ (server-level port) |
| share (config `share`/enterprise cloud) | ref TUI itself runs `unavailable("Sharing")` (tui routes/session/index.tsx:893-897) | ✅-ish (parity = stub in both) |
| title auto-generation (title.ts) | `session/runner/title.rs` + drain fork_title | ✅ |
| title **regeneration** (`""` on update → LLM regen) | handlers/session.rs:472-473 comment: "TODO(phase-9)" — clears instead | 🟡 |
| stats (stats.ts: 436 L, binned activity/model breakdown) | handlers/session.rs:1379-1417 minimal (count + token totals) | 🟡 |
| diff (turn-range diffs, diff.ts:138 L) | handlers/session.rs:887-895 returns `[]` (TODO(phase-9)) | 🟡 |
| generate one-shot (generate.ts, session/generate.ts) | handlers/session.rs:898-907 + misc.rs:72-81 → **503**; engine LLM surface not exposed for one-shots | ❌ (route stub; core runner can do it) |
| shell — user shell run (session.ts:179-222 + session/shell.ts: spawns via Shell service, `session.shell.started/ended`, user-notification) | handlers/session.rs:913-944 admits the **command text as a Synthetic LLM prompt** — the drain treats Synthetic as conversation input (runner/drain.rs:168,310) | ⚠️ **divergent: sends the command to the model instead of executing it** |
| command (session command execution, `command.executed` event) | handlers/session.rs:850-862 → 404 "plugin-registered, none in this build"; `command.executed` event absent from bus registry | 🟡 |

### 1.2 Compaction / context

| Ref | Rust | Class |
|---|---|---|
| manual + auto compaction, estimate context/prompt, native OpenAI-Responses + Anthropic `context_management.edits` trigger (compaction.ts:963 L; runner/llm.ts) | `session/runner/compaction.rs` (1501 L), `RouteLlmClient::can_compact` (runner/mod.rs:160-176), compaction events incl. delta/failed | ✅ |

### 1.3 Execution / retry / restart

| Ref | Rust | Class |
|---|---|---|
| run coordinator, write-ahead claim, interrupt w/ reason + settlement, max-steps, retry scheduling (`session.retry.scheduled`) (execution.ts, run-coordinator.ts, execution/restart.ts: prepareResume/recoverShell/recoverSubagent/resumeSuspended) | `session/execution.rs` (1011 L), runner retry/max_steps, stale compaction+tool-call settle on boot (runner/drain.rs:177-178), continuation recovery | ✅ |

### 1.4 Jobs / background tasks (job.ts)

| Ref ops | Rust (`jobs.rs`, 868 L) | Class |
|---|---|---|
| get/start/wait/block/background/backgroundAll/cancel/pendingBackground/completeBackground (+ shell/subagent recovery records, KV persistence) | all present incl. `cancel_session` | ✅ |

### 1.5 Subagents (tool/plugin/subagent.ts + subagent-job/completion)

| Ref | Rust | Class |
|---|---|---|
| depth guard, agent resolution, model resolution, child execution, background + notification delivery | `subagent.rs` (595 L) + `tool/subagent.rs` (634 L) + `session/subagent_job.rs`/`subagent_completion.rs` + `subagent_jobs_adapter.rs` | ✅ |

### 1.6 Plugin system

| Ref (plugin/*) | Rust | Class |
|---|---|---|
| hook surface: 18 points — session{prompt,context,compaction,generate,title,model.request,http.request,http.response,ws.handshake,ws.send,ws.receive,retry}, tool{execute.before/after}, shell{create.before}, permission{evaluate}, aisdk{sdk,language} (plugin/hooks.ts Domains; plugin/effect/*.ts) | `hooks.rs:859-896` — **all 18** with provider-scoped registration + failable `tool.execute.before` | ✅ |
| plugin host (bun supervisor, module load, source resolution pkg/dir) | oc-plugin-host (supervisor 1423 L) **wired in oc-server** plugins.rs (agents/tools/mcps/skills/hooks/themes merge, Code Mode, `ctx.session.*` port) | ✅ |
| plugin update (plugin/update.ts) | real pinned installer in oc-cli handlers/plugin.rs:160; **server routes `plugin.check/update` are no-ops** (misc.rs:230-246) | 🟡 |
| `command.list` from registered plugin commands | host registers `PluginCommand` (registrations.rs:20) **but** route returns `[]` (agent.rs:62-64) | 🟡 unwired |
| warming plugin (model pre-warm on config `warming`) (plugin/warming.ts) | config key parsed only (normalize.rs:583); no runtime | ❌ |
| optimize plugin — per-model system prompts (gpt/gpt-astra/kimi/meta/trinity/anthropic .txt) + tool pruning (plugin/optimize.ts + system-prompt/*) | absent (grep: no equivalent; base system.txt only, runner/system_prompt.rs) | ❌ |
| built-in provider plugins (30 files: azure, bedrock, vertex, copilot, zenmux, …) | none (see §1.8) | ❌ |
| vcs plugins (git/hg) (plugin/vcs/*) | git via shell-outs only; no hg | 🟡 |
| websearch plugins (exa/firecrawl/tavily/tinyfish/parallel/mcp) | providers implemented (§1.9) but unwired | ⚠️ |

### 1.7 Provider/model catalog + models-dev sync

| Ref | Rust | Class |
|---|---|---|
| models-dev snapshot parse + KV cache + 5-min TTL refresh + digest-skip (models-dev.ts:255-257) | `provider_catalog/refresh.rs` ports all of it — **but no `CatalogSource` impl and no refresh loop is wired anywhere** (grep: CatalogService unused outside tests) → catalog frozen at `models-dev-snapshot.txt` | 🟡 |
| catalog/compat parse, variants (compatibility, effort/budget variants), model resolution | catalog.rs/variant.rs/native.rs/overlay.rs | ✅ |

### 1.8 Providers + auth (credential + integration + oauth + provider plugins)

| Ref | Rust | Class |
|---|---|---|
| credential CRUD/activate (credential.ts:34-53) | handlers/credential.rs (db-backed) | ✅ |
| **Integration host** (integration.ts, 276-L interface): registry, connections (credential/env), `connection.{active,resolve,key,activate,update,remove,status}`, `oauth.{connect,status,complete,cancel}` (auto+code modes, 10-min attempts, scrub loop, refresh ≥5 min before expiry), `command.{connect,status,cancel}`, well-known discovery, branded callback pages (oauth/page.ts) | **absent**: `integration.list` → `[]` (misc.rs:106-109), oauth → errors (misc.rs:290), wellknown add → always-400 (misc.rs:87-94); no IntegrationService in oc-core | ❌ **the whole provider-login pipeline** |
| OAuth provider registrations: opencode/zen (plugin/provider/opencode.ts:88-236, console.opencode.ai), github-copilot device flow (plugin/provider/github-copilot.ts:47-, 4297 L), azure, chatgpt, xai, snowflake, digitalocean, gitlab, poe, openai | none | ❌ |
| GitHub Copilot provider (dedicated transport + models + responses prep) | no copilot binding in facade.rs | ❌ |
| Bedrock (SigV4, converse protocol 892 L), Vertex native, `aisdk:` shims | typed `ProviderError::UnsupportedProtocol` (facade.rs:48-56) — clean error, no support | ❌ (clean failure) |
| provider auth resolution (env vars per catalog `env` list, api keys) | `route/auth.rs` combinators (bearer/header/scheme) + env | ✅ for key-auth providers |

### 1.9 oc-ai (LLM + media)

| Ref (packages/ai/src/protocols, 14.8k L) | Rust (oc-ai/src/protocols, 18.6k L) | Class |
|---|---|---|
| text: anthropic-messages, openai-chat, openai-responses, open-responses (+channel/continuation), gemini, mistral-chat, cohere-chat, openai-compatible, alibaba×3, meta×2, venice, zai, xai-responses, bedrock-converse | anthropic_messages (5420 L), openai_chat (5612 L), openai_responses, open_responses (3322 L), gemini, mistral_chat, cohere_chat, openai_compatible + per-provider chat factories (deepseek/groq/openrouter/venice/zai/alibaba) | ✅ for majors; ❌ bedrock-converse, meta, xai-responses, zai-messages |
| image generation (openai/google/bfl/fal/meta/replicate/stability/xai/zai), video (fal/runway/google/xai), speech (openai/cartesia/deepgram/elevenlabs), transcription (openai/assemblyai/deepgram/elevenlabs/google) — image.ts/media.ts/speech.ts/transcription.ts/video.ts + clients | **no media protocols at all** | ❌ |
| route layer (endpoint/auth/framing/transport/executor), cache-policy, effort-updates, provider-error, sanitize, tool-history | route/* + top-level modules — direct ports | ✅ |

### 1.10 Tools catalog (ref tool/plugin/*.ts = 15 leaves)

| Ref | Rust (tool/*) | Class |
|---|---|---|
| edit, write, read, glob, grep, patch, shell, question, webfetch, skill, subagent, mcp-resource (list/read), opencode (session_rename/session_move/models), websearch, file-diff helper | all present & registered in engine pre-plugins (engine/mod.rs:920-936, 1449+) | ✅ |
| websearch execution | `WebSearch::new(db, Vec::new())` (engine/mod.rs:1613) — **zero providers**; `default_providers()` (websearch_providers.rs:479: exa/tavily/firecrawl/parallel/tinyfish) is dead code; server routes 503/`[]` (misc.rs:288-306) | ⚠️ registered tool that always fails |
| `session_move` opencode-tool | declared stub (opencode_tools.rs:89-116 TODO(wave-integration)) | 🟡 |

### 1.11 MCP

| Ref (mcp/*, 2103 L) | Rust (mcp/*, ~3200 L) | Class |
|---|---|---|
| client (stdio spawn w/ env, streamable-http + legacy session-expiry retry), registry (add/connect/disconnect/remove, reconcile, watch, prompts, instructions, resources, namespaced tools, per-URL serialization) | client.rs (rmcp stdio + StreamableHttpClient w/ retry), registry.rs (1679 L) full op set, elicitation.rs (503 L) | ✅ |
| MCP OAuth (oauth.ts 532 L: CIMD, discovery, PKCE loopback server, refresh dedup) | stored-token bearer only; missing token + 401 → `needs_auth` dead-end; **login flow explicitly deferred** (registry.rs:180-186) | ❌ flow |
| `mcp.prompts.changed` ephemeral event | event type absent (prompts() exists, no publish) | 🟡 |

### 1.12 PTY / persistent PTY

| Ref (pty.ts 317 L; persistent-pty/ 1212 L) | Rust (pty.rs 818 L; persistent_pty.rs 1818 L incl. daemon snapshot/handoff/adoption) | ✅ parity |

### 1.13 Config

| Ref (config/* + schema/config) | Rust (config/*) | Class |
|---|---|---|
| discovery (global/explicit/direct/project/.claude/.agents), normalize overlay + diagnostics, watch plan (dirs + parent-entry watches), ~30 schema sections incl. share/enterprise/warming/media/watcher | discovery.rs (286 L), normalize.rs (3442 L, all keys incl. share/enterprise/warming), watch.rs (617 L) | ✅ |
| config.update (jsonc write preserving comments, config.ts:333-) | jsonc_edit.rs (1278 L) ported; used by **oc-cli** (shell/mcp add) — **server `experimental.config.update` hardcoded 500** (handlers/config.rs:56-66) | ⚠️ core exists, route stubbed |

### 1.14 Events / bus

| Ref (~110 durable+ephemeral types across schema) | Rust (oc-schema registry, 94 types) | 🟡 |
|---|---|---|
| missing types: `command.executed`, `file.edited`, `filesystem.changed`, `reference.updated`, `session.diff`, `session.error`, `session.updated`, `mcp.prompts.changed` (ephemeral), `sdk.plugin.updated`, `wellknown.updated`, `instruction-discovery.updated` | (plus rust-only internal `session.deleted.2` version marker) | — |

### 1.15 Permissions / forms / keybinds

| Ref | Rust | Class |
|---|---|---|
| permission ask/assert/reply/get/list + saved rulesets + config policies | permission.rs (1493 L) + permission/saved.rs | ✅ |
| forms create/get/list/reply/cancel | forms.rs (963 L) | ✅ |
| keybind/leader config | **TUI-side only in both** (tui/src/config/keybind.ts) — not a core gap | n/a |

### 1.16 VCS / worktree

| Ref (vcs.ts + plugin/vcs/{git,hg}; worktree/ 149 L) | Rust | Class |
|---|---|---|
| vcs get/base/branches/diff/init/status + `vcs.branch.updated` events | handlers/project.rs:322-410 — **git subprocess subset** (rev-parse/merge-base/branch/diff); no hg, no jj, no events, diff = raw string hunks | 🟡 |
| worktree list/create/remove/refresh with strategies + `worktree.ready/failed/resolved` events | `worktree_create` **inserts a DB row only — never runs `git worktree add`** (project.rs:259-281 "git-side lands with the vcs lane TODO(phase-9)") | ⚠️ hollow |

### 1.17 Files / images

| Ref | Rust | Class |
|---|---|---|
| file-access resolve/authorize, file-mutation, snapshot, ripgrep | file_access.rs (842 L), file_mutation.rs, snapshot.rs | ✅ |
| image normalize/resize via photon-wasm on prompt attachments (image.ts, session/prompt.ts:140-146) | images pass through as raw base64 (tool/read.rs handles mime only) | ❌ normalize |

### 1.18 Misc services

| Ref | Rust | Class |
|---|---|---|
| shell service (3142 L) + select | shell.rs + shell/scan.rs | ✅ |
| skills, instructions, instruction-discovery, formatter, reference, project, workspace, location activity/lifecycle, wellknown, event-logger, kv, instance, codemode | ported (engine/skills.rs, instructions.rs, formatter.rs, reference_list handler, location*.rs, tool/codemode.rs) | ✅ |
| v1 migration (v1/, database/v1-migration) | v2_migration.rs (730 L) + route | ✅ |
| ACP (packages/cli/src/acp/*) | crates/oc-cli/src/acp/* | ✅ (cli lane) |
| account/enterprise + managed-policy deployment | absent (config keys parsed only) | ❌ (rare) |

---

## 2. TUI audit-B top-10 → core or TUI?

| # | audit-B finding | Traces to | Fix lands in |
|---|---|---|---|
| 1 | 12 dead commands (`/settings /status /mcps /variants /open /connect /pair /debug /share /fork /timeline` docs) | `/mcps` `/status`: core+routes exist (mcp.list, McpRegistry) — **TUI wiring**. `/variants`: variant.rs + model.list exist — **TUI**. `/pair`: pairing.rs exists — **TUI**. `/debug`: local info — **TUI**. `/settings`: config.get ok but **config.update route stubbed 500** — core(server)+TUI. `/connect`: **integration host missing — CORE**. `/fork`: core projector fine, **server handler boundary bug — CORE(server)** + TUI dialog. `/timeline`: messages API — **TUI**. `/share`: stub in ref too — parity (cosmetic). | 6 TUI / 3 core / 1 parity |
| 2 | No Reconnecting/StartupLoading overlays | client connection state | **TUI** |
| 3 | Sidebar hollow (context/MCP/location) | usage events + mcp.list + vcs.get all exist core-side (vcs is git-subset) | **TUI** (mostly) |
| 4 | Getting-started gate `!hasConnectedProvider` + dead ✕ | needs integration list — **CORE** (integration host) for the gate; ✕ fix is TUI | mixed |
| 5 | Transcript detail states (tool cells, compaction rows, revert banner) | tool_output.rs + compaction events exist — **TUI** |
| 6 | Diff viewer unreachable | snapshot diff + file_diff.rs exist core-side | **TUI** |
| 7 | Home update/MCP/plugins-failed cells | mcp/plugin status events exist; updater absent everywhere (ref: client-spawned) | TUI + product decision |
| 8 | Theme token gaps | — | **TUI** |
| 9 | No updater surface | ref updater is CLI-spawned, not core | TUI/CLI |
| 10 | MigrationOverlay / SessionLocationMissing / MCP toasts | migration status route + location.shutdown event exist — **TUI**; MCP needs-auth toast copy is TUI but the **login flow behind "Sign in" is CORE-missing** | mixed |

**Verdict**: the majority of audit-B's dead surfaces are **TUI wiring over working core**; the exceptions that trace to core are: `/connect` + provider onboarding (integration host), `/settings` save path, `/fork` server bug, and MCP sign-in.

---

## 3. Top-10 worst CORE findings (severity: user-daily > integration-critical > rare)

1. **Integration/auth host entirely missing** (ref integration.ts + oauth pages + 10 OAuth provider plugins). No provider login works: `oc auth login` (oc-cli handlers/auth.rs:429 → server stubs), TUI `/connect`, sidebar tokens, MCP OAuth servers all dead-end. First-run blocker. — *user-daily + integration-critical*
2. **`session.shell` sends the command to the LLM instead of executing it** (handlers/session.rs:913-944 admits Synthetic text; ref session.ts:179-222 spawns via Shell service + shell.started/ended events). Latent today (TUI shell-mode missing) but API-visible. — *user-daily once wired*
3. **`session.fork` API never copies history**: handler mis-decodes the `before` boundary (string vs `{type,messageID}` object, ref session-fork.ts) and pre-creates the session row, so the correct `project_fork` projector (projector.rs:571) always fails `ALREADY_PROJECTED`/boundary-not-found — errors are swallowed (`let _ = publish`). Fork = empty session, wrong title. — *user-daily*
4. **websearch tool registered with zero providers** (engine/mod.rs:1613 `Vec::new()`; `default_providers()` dead code; routes 503/empty). Model will call the tool and always fail. — *user-daily*
5. **models-dev catalog refresh ported but never wired** (refresh.rs CatalogService unused; no CatalogSource, no 5-min TTL loop) — new providers/models never appear without a rebuild. — *integration-critical*
6. **GitHub Copilot provider absent** (ref 4.3k lines, device OAuth + custom transport); **Bedrock + Vertex native = typed UnsupportedProtocol** (facade.rs:48-56). Large provider classes unusable. — *integration-critical*
7. **oc-ai media surface missing**: all image/video/speech/transcription protocols + one-shot `generate` (503) + `session.diff` (`[]`) + LLM title regeneration. — *integration-critical (generate/diff) / rare (media)*
8. **`experimental.config.update` hardcoded 500** while jsonc_edit.rs is ported and already used by oc-cli — `/settings` cannot save. — *user-daily*
9. **Plugin runtime gaps**: warming, per-model system-prompt optimization (gpt/claude/kimi/meta/trinity prompts — every model gets the base prompt), plugin check/update no-op routes, command.list returns `[]` despite host registrations, image normalize (photon) missing. — *user-daily (prompt quality) / rare (rest)*
10. **worktree.create never runs git** (DB row pointing at a non-existent dir) + vcs is a git-subprocess subset (no hg, no `vcs.branch.updated` events, raw-string diffs). The subagent worktree workflow (`session_move` + worktree.create) cannot round-trip. — *integration-critical*

Honorable mentions: 11 event types missing from the bus registry (incl. `command.executed`, `filesystem.changed`, `mcp.prompts.changed`); MCP OAuth stored-token-only (`needs_auth` is a dead-end until #1 lands); session stats minimal (binned activity/model breakdown TODO); enterprise/managed-policy absent.
