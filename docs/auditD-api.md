# Audit D — v2 Server HTTP API surface vs `oc-server` (Rust port)

**Scope**: exhaustive inventory of the v2 reference server HTTP API (`/opt/opencode-ref/packages/server` + the protocol route definitions it mounts from `/opt/opencode-ref/packages/protocol`) versus the Rust port (`crates/oc-server`), including routes, request/response shapes, error envelopes, status codes, SSE, WebSocket, auth, CORS, and static hosting. The Web UI (`packages/app`) and both TUIs consume this API; anything that drifts breaks them silently.

**Verdict in one line**: the HTTP route table is at **full parity (141/141 routes, 0 missing, 0 extra paths)** and error envelopes/status codes/CORS/auth match. The real gaps are in the **`/api/event` SSE stream**: **20 of 89 public event types never reach clients**, 6 of them because oc-core *publishes* them and the port's public-event filter silently drops them (`form.*`, `shell.*`).

---

## 1. Reference inventory (v2)

The v2 server does not define routes inline: `packages/server/src/routes.ts:176` mounts an `Api` built by `packages/protocol/src/api.ts:155-194` from 31 `HttpApiGroup`s in `packages/protocol/src/groups/`. Middleware: `Authorization` (all groups) + `SchemaErrorMiddleware` (`api.ts:193-194`). Handlers live in `packages/server/src/handlers/*.ts`.

### 1.1 Route table (141 endpoints)

| # | Method | Path | ID | Ref cite |
|---|--------|------|----|----------|
| **server** (`groups/server.ts`) ||||
| 1 | GET | `/api/info` | server.info | server.ts:42 |
| 2 | POST | `/api/pair` | server.pair | server.ts:53 |
| 3 | GET | `/auth/connect/:code` | server.connect | server.ts:64 |
| **location** (`groups/location.ts`) ||||
| 4 | GET | `/api/location` | location.get | location.ts:36 |
| 5 | POST | `/api/location/reload` | location.reload | location.ts:51 |
| **agent** (`groups/agent.ts`) ||||
| 6 | GET | `/api/agent` | agent.list | agent.ts:10 |
| 7 | GET | `/api/agent/:agentID` | agent.get | agent.ts:24 |
| **plugin** (`groups/plugin.ts`) ||||
| 8 | GET | `/api/plugin` | plugin.list | plugin.ts:10 |
| 9 | POST | `/api/plugin/check` | plugin.check | plugin.ts:24 |
| 10 | POST | `/api/plugin/update` | plugin.update | plugin.ts:40 |
| **session** (`groups/session.ts`, 43 endpoints) ||||
| 11 | GET | `/api/session` | session.list | session.ts:180 |
| 12 | GET | `/api/experimental/session/stats` | session.stats | session.ts:200 |
| 13 | POST | `/api/session` | session.create | session.ts:219 |
| 14 | POST | `/api/experimental/session/import` | session.import | session.ts:242 |
| 15 | GET | `/api/experimental/session/:sessionID/export` | session.export | session.ts:259 |
| 16 | GET | `/api/session/active` | session.active | session.ts:273 |
| 17 | GET | `/api/session/:sessionID` | session.get | session.ts:285 |
| 18 | DELETE | `/api/session/:sessionID` | session.remove | session.ts:298 |
| 19 | POST | `/api/session/:sessionID/fork` | session.fork | session.ts:311 |
| 20 | POST | `/api/session/:sessionID/agent` | session.switchAgent | session.ts:328 |
| 21 | POST | `/api/session/:sessionID/model` | session.switchModel | session.ts:344 |
| 22 | PATCH | `/api/session/:sessionID` | session.update | session.ts:360 |
| 23 | POST | `/api/session/:sessionID/move` | session.move | session.ts:380 |
| 24 | POST | `/api/session/:sessionID/prompt` | session.prompt | session.ts:394 |
| 25 | POST | `/api/session/:sessionID/command` | session.command | session.ts:416 |
| 26 | POST | `/api/experimental/session/:sessionID/skill` | session.skill | session.ts:436 |
| 27 | POST | `/api/session/:sessionID/synthetic` | session.synthetic | session.ts:455 |
| 28 | POST | `/api/session/:sessionID/shell` | session.shell | session.ts:478 |
| 29 | POST | `/api/session/:sessionID/compact` | session.compact | session.ts:498 |
| 30 | POST | `/api/experimental/session/:sessionID/wait` | session.wait | session.ts:518 |
| 31 | POST | `/api/session/:sessionID/revert/stage` | session.revert.stage | session.ts:533 |
| 32 | DELETE | `/api/session/:sessionID/revert` | session.revert.clear | session.ts:549 |
| 33 | POST | `/api/session/:sessionID/revert/commit` | session.revert.commit | session.ts:558 |
| 34 | GET | `/api/session/:sessionID/context` | session.context | session.ts:567 |
| 35 | GET | `/api/session/:sessionID/diff` | session.diff | session.ts:580 |
| 36 | GET | `/api/session/:sessionID/inbox` | session.inbox.list | session.ts:605 |
| 37 | DELETE | `/api/session/:sessionID/inbox/:inboxID` | session.inbox.cancel | session.ts:619 |
| 38 | PATCH | `/api/session/:sessionID/inbox/:inboxID` | session.inbox.update | session.ts:632 |
| 39 | GET | `/api/experimental/session/:sessionID/instructions/entries` | instructions.entry.list | session.ts:646 |
| 40 | PUT | `/api/experimental/session/:sessionID/instructions/entries/:key` | instructions.entry.put | session.ts:665 |
| 41 | DELETE | `/api/experimental/session/:sessionID/instructions/entries/:key` | instructions.entry.remove | session.ts:686 |
| 42 | POST | `/api/session/:sessionID/generate` | session.generate | session.ts:706 |
| 43 | GET | `/api/experimental/session/:sessionID/log` (**SSE**) | session.log | session.ts:724 |
| 44 | POST | `/api/session/:sessionID/interrupt` | session.interrupt | session.ts:744 |
| 45 | POST | `/api/session/:sessionID/background` | session.background | session.ts:765 |
| 46 | GET | `/api/session/:sessionID/message/:messageID` | session.message.get | session.ts:781 |
| 47 | GET | `/api/session/:sessionID/form` | session.form.list | session.ts:794 |
| 48 | POST | `/api/session/:sessionID/form` | session.form.create | session.ts:809 |
| 49 | GET | `/api/session/:sessionID/form/:formID` | session.form.get | session.ts:825 |
| 50 | POST | `/api/session/:sessionID/form/:formID/reply` | session.form.reply | session.ts:840 |
| 51 | DELETE | `/api/session/:sessionID/form/:formID` | session.form.cancel | session.ts:856 |
| 52 | PUT | `/api/session/:sessionID/environment` | session.environment | session.ts:872 |
| 53 | POST | `/api/session/:sessionID/view` | session.view | session.ts:886 |
| **message** (`groups/message.ts`) ||||
| 54 | GET | `/api/session/:sessionID/message` | session.messages | message.ts:68 |
| **model** (`groups/model.ts`) ||||
| 55 | GET | `/api/model` | model.list | model.ts:10 |
| 56 | GET | `/api/model/default` | model.default | model.ts:26 |
| **generate** (`groups/generate.ts`) ||||
| 57 | POST | `/api/experimental/generate` | generate.text | generate.ts:8 |
| **provider** (`groups/provider.ts`) ||||
| 58 | GET | `/api/provider` | provider.list | provider.ts:10 |
| 59 | GET | `/api/provider/:providerID` | provider.get | provider.ts:25 |
| **integration** (`groups/integration.ts`, 11) ||||
| 60 | GET | `/api/integration` | integration.list | integration.ts:16 |
| 61 | GET | `/api/integration/:integrationID` | integration.get | integration.ts:30 |
| 62 | POST | `/api/experimental/integration/wellknown` | wellknown.add | integration.ts:46 |
| 63 | POST | `/api/integration/:integrationID/connect/key` | connect.key | integration.ts:62 |
| 64 | POST | `/api/integration/:integrationID/connect/oauth` | oauth.connect | integration.ts:83 |
| 65 | GET | `/api/integration/:integrationID/connect/oauth/:attemptID` | oauth.status | integration.ts:104 |
| 66 | POST | `/api/integration/:integrationID/connect/oauth/:attemptID/complete` | oauth.complete | integration.ts:120 |
| 67 | DELETE | `/api/integration/:integrationID/connect/oauth/:attemptID` | oauth.cancel | integration.ts:141 |
| 68 | POST | `/api/integration/:integrationID/connect/command` | command.connect | integration.ts:156 |
| 69 | GET | `/api/integration/:integrationID/connect/command/:attemptID` | command.status | integration.ts:176 |
| 70 | DELETE | `/api/integration/:integrationID/connect/command/:attemptID` | command.cancel | integration.ts:191 |
| **mcp** (`groups/mcp.ts`) ||||
| 71 | GET | `/api/mcp` | mcp.list | mcp.ts:10 |
| 72 | PUT | `/api/experimental/mcp/:server` | mcp.add | mcp.ts:24 |
| 73 | DELETE | `/api/experimental/mcp/:server` | mcp.remove | mcp.ts:42 |
| 74 | POST | `/api/experimental/mcp/:server/connect` | mcp.connect | mcp.ts:58 |
| 75 | POST | `/api/experimental/mcp/:server/disconnect` | mcp.disconnect | mcp.ts:74 |
| 76 | GET | `/api/mcp/resource` | mcp.resource.catalog | mcp.ts:90 |
| **credential** (`groups/credential.ts`) ||||
| 77 | GET | `/api/credential` | credential.list | credential.ts:8 |
| 78 | POST | `/api/credential` | credential.create | credential.ts:19 |
| 79 | PATCH | `/api/credential/:credentialID` | credential.update | credential.ts:33 |
| 80 | POST | `/api/credential/:credentialID/activate` | credential.activate | credential.ts:48 |
| 81 | DELETE | `/api/credential/:credentialID` | credential.remove | credential.ts:61 |
| **project** (`groups/project.ts`) ||||
| 82 | GET | `/api/project` | project.list | project.ts:11 |
| 83 | PATCH | `/api/project/:projectID` | project.update | project.ts:22 |
| **form** (`groups/form.ts`) ||||
| 84 | GET | `/api/form` | form.list | form.ts:12 |
| **permission** (`groups/permission.ts`) ||||
| 85 | GET | `/api/permission/request` | permission.request.list | permission.ts:23 |
| 86 | GET | `/api/permission/saved` | permission.saved.list | permission.ts:37 |
| 87 | DELETE | `/api/permission/saved/:id` | permission.saved.remove | permission.ts:49 |
| 88 | POST | `/api/session/:sessionID/permission` | session.permission.create | permission.ts:63 |
| 89 | GET | `/api/session/:sessionID/permission` | session.permission.list | permission.ts:89 |
| 90 | GET | `/api/session/:sessionID/permission/:requestID` | session.permission.get | permission.ts:102 |
| 91 | POST | `/api/session/:sessionID/permission/:requestID/reply` | session.permission.reply | permission.ts:117 |
| **fs** (`groups/fs.ts`) ||||
| 92 | GET | `/api/fs/read/*` (raw bytes) | fs.read | fs.ts:32 |
| 93 | GET | `/api/fs/list` | fs.list | fs.ts:47 |
| 94 | GET | `/api/fs/find` | fs.find | fs.ts:62 |
| 95 | POST | `/api/experimental/fs/write` (raw body) | fs.write | fs.ts:76 |
| **command / skill / rpc** ||||
| 96 | GET | `/api/command` | command.list | command.ts:9 |
| 97 | GET | `/api/skill` | skill.list | skill.ts:9 |
| 98 | POST | `/api/rpc/:rpcID/:method` | rpc.call | rpc.ts:14 |
| **event** ||||
| 99 | GET | `/api/event` (**SSE**) | event.subscribe | event.ts:44 |
| **pty** (`groups/pty.ts`) ||||
| 100 | GET | `/api/pty` | pty.list | pty.ts:23 |
| 101 | POST | `/api/pty` | pty.create | pty.ts:37 |
| 102 | GET | `/api/pty/:ptyID` | pty.get | pty.ts:52 |
| 103 | PUT | `/api/pty/:ptyID` | pty.update | pty.ts:68 |
| 104 | DELETE | `/api/pty/:ptyID` | pty.remove | pty.ts:85 |
| 105 | POST | `/api/pty/:ptyID/connect-token` | pty.connectToken | pty.ts:101 |
| 106 | GET | `/api/pty/:ptyID/connect` (**WebSocket**) | pty.connect | pty.ts:120 |
| **persistent-pty** (`groups/persistent-pty.ts`, 11) ||||
| 107 | GET | `/api/experimental/session/:sessionID/terminal/read` | persistentPty.read | persistent-pty.ts:23 |
| 108 | GET | `/api/experimental/session/:sessionID/terminal` | persistentPty.list | persistent-pty.ts:39 |
| 109 | POST | `/api/experimental/session/:sessionID/terminal` | persistentPty.create | persistent-pty.ts:46 |
| 110 | POST | `/api/experimental/persistent-pty/shutdown` | persistentPty.shutdown | persistent-pty.ts:54 |
| 111 | POST | `/api/experimental/persistent-pty/handoff` | persistentPty.handoff | persistent-pty.ts:60 |
| 112 | GET | `/api/experimental/persistent-pty/:ptyID` | persistentPty.get | persistent-pty.ts:66 |
| 113 | PUT | `/api/experimental/persistent-pty/:ptyID` | persistentPty.update | persistent-pty.ts:73 |
| 114 | GET | `/api/experimental/persistent-pty/:ptyID/snapshot` | persistentPty.snapshot | persistent-pty.ts:81 |
| 115 | DELETE | `/api/experimental/persistent-pty/:ptyID` | persistentPty.remove | persistent-pty.ts:88 |
| 116 | POST | `/api/experimental/persistent-pty/:ptyID/connect-token` | persistentPty.connectToken | persistent-pty.ts:95 |
| 117 | GET | `/api/experimental/persistent-pty/:ptyID/connect` (**WebSocket**) | persistentPty.connect | persistent-pty.ts:103 |
| **shell** (`groups/shell.ts`) ||||
| 118 | GET | `/api/shell` | shell.list | shell.ts:10 |
| 119 | POST | `/api/shell` | shell.create | shell.ts:24 |
| 120 | GET | `/api/shell/:id` | shell.get | shell.ts:40 |
| 121 | GET | `/api/shell/:id/output` | shell.output | shell.ts:56 |
| 122 | DELETE | `/api/shell/:id` | shell.remove | shell.ts:72 |
| **reference / worktree** ||||
| 123 | GET | `/api/reference` | reference.list | reference.ts:24 |
| 124 | GET | `/api/worktree` | worktree.list | worktree.ts:23 |
| 125 | POST | `/api/worktree` | worktree.create | worktree.ts:37 |
| 126 | DELETE | `/api/worktree` (with JSON body) | worktree.remove | worktree.ts:51 |
| 127 | POST | `/api/worktree/refresh` | worktree.refresh | worktree.ts:65 |
| **vcs** (`groups/vcs.ts`) ||||
| 128 | POST | `/api/vcs/init` | vcs.init | vcs.ts:36 |
| 129 | GET | `/api/vcs` | vcs.get | vcs.ts:52 |
| 130 | GET | `/api/vcs/base` | vcs.base | vcs.ts:66 |
| 131 | GET | `/api/vcs/status` | vcs.status | vcs.ts:82 |
| 132 | GET | `/api/vcs/branch` | vcs.branch.list | vcs.ts:96 |
| 133 | GET | `/api/vcs/diff` | vcs.diff | vcs.ts:110 |
| **debug / migration / websearch / config** ||||
| 134 | GET | `/api/debug/location` | debug.location.list | debug.ts:8 |
| 135 | DELETE | `/api/debug/location` | debug.location.evict | debug.ts:19 |
| 136 | GET | `/api/experimental/migration/v1` | migration.v1.status | migration.ts:18 |
| 137 | GET | `/api/websearch/provider` | websearch.providers | websearch.ts:10 |
| 138 | POST | `/api/websearch` | websearch.query | websearch.ts:25 |
| 139 | GET | `/api/config` | config.get | config.ts:9 |
| 140 | GET | `/api/config/shell` | config.shells | config.ts:24 |
| 141 | PATCH | `/api/experimental/config` | config.update | config.ts:35 |

### 1.2 Non-protocol HTTP surface (ref)

| Surface | Behavior | Ref cite |
|---|---|---|
| `GET /openapi.json` | Generated OpenAPI doc (`openapiPath`) | routes.ts:176 |
| Pre-ready `/api/info` | Served before app ready with auth; 200/500/503 + `retry-after: 1` | process.ts:172-201 |
| Not-ready other routes | 503 envelope `{code: service_starting\|service_stopping}` / `{code:"service_failed", message, action}` | process.ts:204-216 |
| Static web UI | **Not in packages/server** — served by the CLI (`serveUI`: SPA fallback to `index.html`, `_assets/*`+`icons/*` misses 404 no-store) | cli/src/services/web-ui.ts:26-40 |
| SSE `/api/event` framing | `data: {json}\n\n`; initial synthetic `server.connected` `{id, type, data:{}}`; `: heartbeat\n\n` every 15s; headers `Cache-Control: no-cache, no-transform`, `X-Accel-Buffering: no`, `X-Content-Type-Options: nosniff` | handlers/event.ts:14-40, event-feed.ts:29-31 |
| Session log SSE | `data:` frames of `SessionEvent.Durable ∪ EventLog.Synced` (`{type:"log.synced", aggregateID, seq?}` marker) | session.ts:724-742, schema/event-log.ts:13 |
| Compression + CORS | `HttpMiddleware.compression()`; CORS allowlist localhost/127.0.0.1/`oc://renderer`/tauri hosts/`*.opencode.ai`/opts.cors, `max-age 86400` | process.ts:70-71, cors.ts:105-136 |

### 1.3 Auth surface (ref)

- **Basic**: `Authorization: Basic base64(user:pass)`, fixed username `opencode`; password OR session token accepted as the password slot (auth.ts:33-37).
- **`auth_token` query param**: base64 `user:pass` (priority over header) — middleware/authorization.ts:147-174.
- **Session cookie**: `opencode_session_${port}` (port of Host), HMAC-signed `expires.signature`, 30-day TTL, same-origin Origin check — auth.ts:40-66, authorization.ts:199-206.
- **Pairing**: `POST /api/pair` issues a 5-min single-use code; `GET /auth/connect/:code` redeems it — browsers get cookie + redirect to `/` (HTML accept) or text 401 on bad code; API clients get `{token}` — pairing.ts:69-102, handlers/server.ts:34-55.
- **Auth bypasses**: pairing-connect URLs, PTY/persistent-PTY connect URLs carrying a `ticket` query (handlers consume the ticket) — authorization.ts:216-226, groups/pty.ts:13-19, persistent-pty.ts:12-16.
- **401 shape**: `{_tag:"UnauthorizedError", message:"Authentication required"}`; `www-authenticate: Basic realm="Secure Area"` only when `sec-fetch-mode` absent or `navigate` — authorization.ts:148,176-191.
- **PTY connect tokens**: `POST .../connect-token` requires header `x-opencode-ticket: 1` else 403; issues 60s single-use ticket — groups/pty.ts:9-11, handlers/pty.ts:122-139.
- **Location resolution**: `?location[directory]=` deepObject query and `x-opencode-directory` header (URI-encoded) — groups/location.ts:6-27, server/src/location.ts:61.

### 1.4 Error model (ref)

Tagged JSON envelopes `{"_tag": "<Class>", ...camelCase fields}`; statuses: 400 `InvalidRequestError`(message,kind?,field?) / `RpcError` / `WorktreeError`({name, data{message, forceRequired?}}), 401 `UnauthorizedError`, 403 `ForbiddenError`, 404 `*NotFoundError` (Location/Session/Message/Form/Skill/Provider/Agent/Project/File/McpServer/Command/Integration/Attempt/Method/Permission), 409 `ConflictError`/`SessionBusyError`, 500 `UnknownError`/`CommandExecutionError`/`RpcInternalError`, 501 `VcsInitNotSupportedError` (vcs.ts:15-19), 503 `ServiceUnavailableError` — protocol/src/errors.ts:1-200. Schema rejections collapse to `InvalidRequestError` with `kind` (server/src/middleware/schema-error.ts:244-250). Unmatched API routes → empty 404.

---

## 2. Rust port inventory (`crates/oc-server`)

- **Route table**: `src/routes.gen.rs` — 141 entries (`OPERATION_COUNT = 141`, asserted by `tests/handlers_route_coverage.rs:122`), each `{method, path, client_method}`; mounted by `src/handlers/mod.rs:37-58` (one module per namespace; `attachment.rs` is a library module, no routes).
- **Dispatch/outerware**: `src/app.rs` — CORS outermost → compression → dispatch gate (readiness 503s, pre-ready `/api/info`) → auth → schema errors → location (`src/middleware.rs:111,223`).
- **SSE**: `src/sse.rs` — `/api/event` (`event_response`:458) and `/api/experimental/session/:id/log` (`event_response` log variant:457+), framing `data: {json}\n\n`, initial `server.connected`, 15s `: heartbeat`, identical SSE headers (`sse.rs:335-349`).
- **Errors**: `src/errors.rs` — `_tag` envelopes, 5xx logging, readiness `{code}` envelopes (`errors.rs:489`), `WorktreeError` (`handlers/common.rs:183-192`), `VcsInitNotSupportedError` 501 (`handlers/common.rs:173-181`).
- **Auth**: `src/auth.rs` — Basic/`auth_token`/cookie/verify-session/`www-authenticate` gating (`auth.rs:245-307`); pairing `src/pairing.rs`; PTY tickets `src/tickets.rs`; connect-token `x-opencode-ticket: 1` gate (`handlers/pty.rs:154-176`); WebSocket upgrades for pty + persistent-pty connect.
- **Static UI**: `src/assets.rs` + `src/app_archive.rs` (port of `cli/services/web-ui.ts`), `/openapi.json` sha-pinned from `tools/openapi-gen/openapi.json` (`assets.rs:21-29,338`).

## 3. Diff — per endpoint classification

### 3.1 HTTP routes: **141/141 parity, 0 missing, 0 extra**

Every ref endpoint in §1.1 exists in `routes.gen.rs` with the same method and path (verified by set-comparison of both tables; also enforced by `tests/handlers_route_coverage.rs`). `/openapi.json` and the static web UI are present on both sides (ref: server routes.ts:176 + CLI web-ui.ts; ours: assets.rs).

### 3.2 Shape spot-checks (all parity)

| Check | Ref | Ours | Result |
|---|---|---|---|
| `GET /api/session/:id/message` default limit | 50 (handlers/message.ts:9) | `DEFAULT_LIMIT = 50` (handlers/session.rs:36) | parity |
| message cursor rule | cursor XOR order → `InvalidCursorError` 400 | `handlers/message.rs:57-68` | parity |
| `session.list` envelope | `{data, cursor{previous?,next?}}` | session.rs list handler | parity |
| `session.prompt` | `{data: SessionInbox.User}`, busy → 409 `ConflictError` | `handlers/session.rs` prompt + `inbox_error:137-148` | parity |
| `session.interrupt` body | bare `{interrupted: bool}` (no data envelope) | `handlers/session.rs:644` | parity |
| `project.list` | bare array (no envelope) | `handlers/project.rs:68` | parity |
| Location-scoped lists | `{location:{directory}, data}` | `json_location_data` (handlers/common.rs:227) | parity |
| `noContent` endpoints | 204 truly empty | `no_content()` (common.rs:232) | parity |
| DELETE with body (`worktree.remove`) | payload `Worktree.RemoveInput` | axum `Json` extractor (project.rs:16) | parity |
| `fs.read` | raw bytes + mime | `bytes_response` (common.rs:237) | parity |
| `WorktreeError` 400 / `VcsInitNotSupportedError` 501 | errors.ts / vcs.ts:15 | common.rs:173-192 | parity |
| readiness 503s | `{code: service_*}` + `retry-after:1` | `info.rs:58-71`, errors.rs:489 | parity |
| CORS allowlist + max-age | cors.ts:105-136 | middleware.rs:27-110 | parity |
| SSE headers/heartbeat/connected frame | handlers/event.ts:14-40 | sse.rs:335-357 | parity |
| log SSE `log.synced` marker | `{type, aggregateID, seq?}` | oc-schema events.rs `LogSynced` | parity |

### 3.3 Partial — shape drift found

| Endpoint | Drift | Severity |
|---|---|---|
| `GET /api/info` | Ref body includes `capabilities: {persistentPty: boolean}` (handlers/server.ts:23-32); ours omits the field entirely (`info.rs:29-34`). Schema marks it optional, so clients decode — but capability gating (`PersistentPty.available`) can never see `false`. | SDK-critical (low) |
| `GET /api/event` (SSE) | **20 public event types missing from the stream** (§3.4) | app-critical |
| `x-opencode-build` header | Ours stamps `x-opencode-build` on `/api/info` (`info.rs:14-19,36-38`); ref has no such header. Extra response header — harmless to clients, but a spec deviation. | internal (spec review) |

### 3.4 SSE event-type diff (`/api/event`)

Ref public set = `EventManifest.ServerDefinitions` (schema/src/event-manifest.ts:71-81) + synthesized `server.connected` (groups/event.ts:23-36) + any `rpc.*`. Ours = `is_public_event` (oc-server/src/sse.rs:157-166) = `server.connected` + `registry::by_type` (oc-core/src/bus/registry.rs:266-339: 51 session events + `worktree.resolved` + 15 non-session) + `rpc.*` + `announce::SERVICE_FRAMES` (9 types, announce.rs:71-82).

Session events (51/51), `server.connected`, `rpc.*`, `location.shutdown`, `config.updated`, `permission.asked/replied`, `project.updated`, `mcp.tools/resources/status.changed`, `pty.*` (4), `persistent-pty.added/removed`, `worktree.resolved`, `integration/provider/model/agent/command/skill/websearch/reference/plugin.updated` — **all parity**.

**Missing (never delivered to SSE clients):**

| Event type | Ref def | Published by oc-core? | Consumer impact |
|---|---|---|---|
| `form.created` | schema/form.ts (Event.Definitions) | **YES** (oc-core/src/forms.rs:161) but dropped by `is_public_event` | **App listens** (`app/src/session/requests/websearch.ts:134`); interactive forms never appear via stream |
| `form.replied` | same | **YES** (forms.rs:247) dropped | form-state refresh |
| `form.cancelled` | same | **YES** (forms.rs:273) dropped | form-state refresh |
| `shell.created` | schema/shell.ts | **YES** (oc-core/src/shell.rs:593) dropped | live shell output views |
| `shell.exited` | same | **YES** (shell.rs) dropped | shell exit badges |
| `shell.deleted` | same | **YES** (shell.rs) dropped | shell list cleanup |
| `session.status` | session-status-event.ts | no | **App listens** (`app/src/session/usage-exceeded-dialogs.tsx:96`); busy/idle reactivity degraded |
| `session.idle` | session-status-event.ts | no (only projected internally, projector.rs:501) | TUI-critical per ref manifest comment (event-manifest.ts:74-77) |
| `filesystem.changed` | filesystem.ts | no | **App listens** (`app/src/workspaces/files/model.tsx:236`) — web file tree stops live-updating |
| `worktree.updated` | worktree.ts | no | **App listens** (`app/src/runtime/server/sync.tsx:247`, `new-session/workspace/controller.ts:97`) |
| `credential.updated` | credential.ts | no | SDK: credential cache refresh |
| `credential.switched` | credential.ts | no | SDK |
| `models-dev.refreshed` | models-dev.ts | no | SDK |
| `tui.prompt.append` | tui-event.ts | no | ref-TUI remote-control surface |
| `tui.command.execute` | tui-event.ts | no | ref-TUI |
| `tui.toast.show` | tui-event.ts | no (schema exists, oc-schema events.rs:359) | ref-TUI |
| `tui.session.select` | tui-event.ts | no | ref-TUI |
| `installation.updated` | installation-event.ts | no (schema exists, events.rs:261) | update toast |
| `installation.update-available` | installation-event.ts | no (schema exists, events.rs:264) | update toast |
| `vcs.branch.updated` | vcs-event.ts | no (schema exists, events.rs:169) | branch indicator refresh |

**Extra (ours emits, ref public stream does not):** `mcp.prompts.changed` (registry.rs:307; ref `ServerDefinitions` includes only mcp.tools/resources/status.changed, event-manifest.ts:79-80). Harmless to ref clients but a spec deviation — flag for spec review.

### 3.5 Web UI (`packages/app`) cross-check

Every `api.*` call the app makes (enumerated: config.get/shells/update, credential.remove, file.list/read/find, `/api/experimental/fs/write` (attachments/destination.ts:30), integration.get/connect/oauth*, location.get, mcp.list/connect/disconnect, message.list, permission.request(+reply), plugin.list, project.list/update, server.info, session.{list,get,active,update,remove,move,inbox,interrupt,background,wait,command,shell,revert,switchAgent,switchModel,form}, session.prompt (composer/submit.ts:526), shell.output/remove, vcs.status/diff, websearch.providers, worktree.create, `/api/event` SSE) **exists in our route table** — no route-level web-UI breakage. The breakage risk is concentrated in the missing SSE types above (§3.4).

## 4. Top-10 worst findings (severity-ranked)

1. **`form.created/replied/cancelled` dropped from SSE** — oc-core publishes them; `is_public_event` (sse.rs:157-166) filters them out. The web app's form-elicitation flow listens on the event stream (websearch.ts:134). Interactive forms (MCP elicitation, permission-style asks) never reach the UI in real time. *app-critical*. Fix: add the 3 form types (+ shell, see #2) to the registry/`SERVICE_FRAMES`.
2. **`filesystem.changed` never emitted** — app file watcher depends on it (workspaces/files/model.tsx:236); without it the web workspace tree only updates on refetch. *app-critical*.
3. **`session.status` never emitted** — app usage-exceeded dialog + busy-state listeners (usage-exceeded-dialogs.tsx:96); combined with missing `session.idle`, remote clients lose push-based busy/idle transitions (they still have `GET /api/session/active`). *app-critical/TUI-critical*.
4. **`worktree.updated` never emitted** — app worktree sync (sync.tsx:247) and new-session workspace controller rely on it. *app-critical*.
5. **`shell.created/exited/deleted` published-but-dropped** — same filter bug as #1; breaks live shell output UIs (app calls `shell.output`/`shell.remove`). *TUI/SDK-critical*.
6. **`session.idle` never emitted** — the ref manifest explicitly ships `SessionStatusEvent` "the TUI consumes from the public stream" (event-manifest.ts:74-77). *TUI-critical* (our native TUI bypasses SSE, so this hits remote/SDK TUI consumers only).
7. **`vcs.branch.updated` never emitted** — branch indicators go stale after commits/checkouts. *SDK-critical*.
8. **`credential.updated/switched` never emitted** — SDK credential caches stale after OAuth connect flows (the endpoints themselves are at parity). *SDK-critical*.
9. **`/api/info` omits `capabilities.persistentPty`** — clients cannot feature-gate persistent PTY; the field is optional in the ref schema so nothing hard-fails. *SDK-critical (low)*.
10. **Extra surface to spec-review**: `x-opencode-build` header on `/api/info` (ours only) and `mcp.prompts.changed` on the public stream (ref doesn't ship it publicly). Both benign; document or remove. *internal*.

## 5. Counts

| Metric | Value |
|---|---|
| Ref HTTP routes (protocol) | **141** (+ `/openapi.json`, pre-ready `/api/info`, CLI static UI) |
| Ours HTTP routes | **141** (+ `/openapi.json`, static UI) — enforced by `handlers_route_coverage` test |
| Missing routes | **0** |
| Partial (shape drift) | **2 endpoints** (`/api/info` capabilities; `/api/event` event coverage) + 2 spec-review extras (header, event type) |
| Extra routes | **0** |
| Ref SSE public event types | 89 (51 session + 38 non-session incl. `server.connected`) |
| Ours SSE public event types | 70 |
| SSE types missing | **20** (6 published-but-filtered: `form.*`, `shell.*`; 14 never published) |
| SSE types extra | 1 (`mcp.prompts.changed`) |

---

*Audit method: full read of `packages/protocol/src/groups/*.ts` (29 group files), `packages/server/src/{routes,process,auth,pairing,cors,event-feed}.ts`, `middleware/*`, `handlers/{server,event,pty,message}.ts`, `schema/src/event-manifest.ts` + per-domain event definitions; Rust side `routes.gen.rs`, `handlers/**`, `sse.rs`, `errors.rs`, `auth.rs`, `middleware.rs`, `info.rs`, `assets.rs`, `oc-core/src/bus/registry.rs`, `announce.rs`; app client usage enumerated from `packages/app/src/**` (`api.*` calls, `event.on(...)`, raw `"/api/..."` fetches).*
