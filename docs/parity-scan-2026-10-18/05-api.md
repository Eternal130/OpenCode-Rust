# Parity Scan 05 — HTTP API / 客户端 / 协议 / SDK 子系统

- 日期：2026-10-18
- 参考：`/opt/opencode-ref`（opencode v2.0.24, TypeScript）
- 目标：`/opt/opencode-rust`（Rust 移植）
- 范围（ref）：`packages/server/**`、`packages/protocol/**`、`packages/sdk/**`、`packages/httpapi-codegen/**`、`packages/core/src/rpc.ts`、`core/src/event/**`、schema `event-manifest`/`durable-event-manifest`、`packages/client`（生成客户端）
- 范围（rust）：`crates/oc-server/**`、`crates/oc-client/**`、`tools/openapi-gen/openapi.json`、`oc-schema`/`oc-core` 事件类型
- 方法：全文阅读两侧关键源文件 + OpenAPI 结构化 diff（逐 path/逐 schema）+ 事件集合程序化比对 + SDK 方法清单程序化比对；对照 `docs/auditD-api.md`、`docs/reposcan-findings.md`、`docs/auditE-core.md`、`ROADMAP.md` 标注 KNOWN/NEW。

## 0. 总览（一句话结论）

**路由层完全对齐**：两侧 OpenAPI 文档 **sha256 逐字节一致**（`0612418…`），141 个操作 0 缺失 0 多余；错误信封、认证、CORS、SSE 帧/心跳/公共事件集合（BE-1 之后）、客户端 SSE 解码、生命周期常量均达到高保真。**真正的差距集中在四处**：(1) 普通 PTY WebSocket 的 meta 帧协议（顺序/编码/关闭码三重偏差，v2 客户端无法恢复游标）；(2) persistent-PTY `resized` 帧缺 `checkpoint`；(3) 两个 WS connect 的票据消费少了 Origin 白名单门；(4) plugin.check/update、/api/rpc、worktree.refresh、session.stats 分桶、vcs.diff 文件级解析等已登记的 phase-9 桩。

| 度量 | 值 |
|---|---|
| OpenAPI 文档 | **逐字节一致**（ref `packages/protocol/openapi.json` ≡ rust `tools/openapi-gen/openapi.json`，sha256 `0612418392961ef22f0337ce0e1e520270917665fc85f29c4af1c94d1e1873ee`，含 components/schemas 全量 diff = 0 差异） |
| HTTP 操作数 | ref **141** / rust **141**（`routes.gen.rs:12 OPERATION_COUNT=141`，141 个 `.route()` 挂载） |
| 缺失路由 | **0** |
| 多余路由 | **0**（`/openapi.json`、静态资源、pre-ready `/api/info` 为双侧共有的非协议面） |
| SSE 公共事件类型 | ref **95**（51 session + 44 非 session 含 `server.connected`）/ rust **98**；缺失 **0**，多余 **3**（`mcp.tools.changed`、`mcp.prompts.changed`、`session.diff`） |
| v2 SDK 方法 | **139**（不含 2 个 WS upgrade 操作）/ rust client **141**（含 `pty::connect`、`persistent_pty::connect` 两个布尔 GET） |
| 差异条目 | S1 ×4（均为 NEW，PTY WS 族：F1-F4）、S2 ×18（KNOWN ×8：F5-F12；NEW ×10：F13-F22）、footnote ×4 |

> 注：auditD 记 ref 公共事件为 89 个（51 session + 38 非 session）；本次按 `event-manifest.ts:71-81` 逐文件重算为 95（非 session 侧多数源于 foundation/feature 清单展开）。数值口径差异不影响结论。

---

## 1. 路由级映射表（v2 → Rust）

来源：ref `packages/protocol/openapi.json` × rust `crates/oc-server/src/routes.gen.rs`（由同一份 pinned 规范生成，`tools/openapi-gen/tests/gen.rs` 断言 141/141 与 sha 一致；`oc-server` 内 `handlers_route_coverage` 测试强制挂载覆盖）。「Rust client 方法」列为 `routes_gen.client_method`；与 v2 SDK 命名不同处标注 v2 名称。状态列：OK = 同名（snake→camel 等价）；⚠ 命名差异 = 线上等价、仅 Rust 命名法不同。

| # | Method | Path | operationId | Rust client 方法 | 状态 |
|---|--------|------|-------------|------------------|------|
| 1 | GET | `/api/agent` | agent.list | `agent::list` | OK |
| 2 | GET | `/api/agent/{agentID}` | agent.get | `agent::get` | OK |
| 3 | GET | `/api/command` | command.list | `command::list` | OK |
| 4 | GET | `/api/config` | config.get | `config::get` | OK |
| 5 | GET | `/api/config/shell` | config.shells | `config::shells` | OK |
| 6 | GET | `/api/credential` | credential.list | `credential::list` | OK |
| 7 | POST | `/api/credential` | credential.create | `credential::create` | OK |
| 8 | DELETE | `/api/credential/{credentialID}` | credential.remove | `credential::remove` | OK |
| 9 | PATCH | `/api/credential/{credentialID}` | credential.update | `credential::update` | OK |
| 10 | POST | `/api/credential/{credentialID}/activate` | credential.activate | `credential::activate` | OK |
| 11 | DELETE | `/api/debug/location` | debug.location.evict | `debug::location::evict` | OK |
| 12 | GET | `/api/debug/location` | debug.location.list | `debug::location::list` | OK |
| 13 | GET | `/api/event` | event.subscribe | `event::subscribe` | OK |
| 14 | PATCH | `/api/experimental/config` | experimental.config.update | `config::update` | OK |
| 15 | POST | `/api/experimental/fs/write` | experimental.fs.write | `fs::write` → v2: `file.write` | ⚠ 命名差异 |
| 16 | POST | `/api/experimental/generate` | experimental.generate.text | `generate::text` | OK |
| 17 | POST | `/api/experimental/integration/wellknown` | experimental.integration.wellknown.add | `integration::wellknown_add` → v2: `integration.wellknown.add` | ⚠ 命名差异 |
| 18 | DELETE | `/api/experimental/mcp/{server}` | experimental.mcp.remove | `mcp::remove` | OK |
| 19 | PUT | `/api/experimental/mcp/{server}` | experimental.mcp.add | `mcp::add` | OK |
| 20 | POST | `/api/experimental/mcp/{server}/connect` | experimental.mcp.connect | `mcp::connect` | OK |
| 21 | POST | `/api/experimental/mcp/{server}/disconnect` | experimental.mcp.disconnect | `mcp::disconnect` | OK |
| 22 | GET | `/api/experimental/migration/v1` | experimental.migration.v1.status | `migration::status` → v2: `migration.v1.status` | ⚠ 命名差异 |
| 23 | POST | `/api/experimental/persistent-pty/handoff` | server.experimental.persistentPty.handoff | `persistent_pty::handoff` → v2: `experimental.persistentPty.handoff` | ⚠ 命名差异 |
| 24 | POST | `/api/experimental/persistent-pty/shutdown` | server.experimental.persistentPty.shutdown | `persistent_pty::shutdown` → v2: `experimental.persistentPty.shutdown` | ⚠ 命名差异 |
| 25 | DELETE | `/api/experimental/persistent-pty/{ptyID}` | server.experimental.persistentPty.remove | `persistent_pty::remove` → v2: `experimental.persistentPty.remove` | ⚠ 命名差异 |
| 26 | GET | `/api/experimental/persistent-pty/{ptyID}` | server.experimental.persistentPty.get | `persistent_pty::get` → v2: `experimental.persistentPty.get` | ⚠ 命名差异 |
| 27 | PUT | `/api/experimental/persistent-pty/{ptyID}` | server.experimental.persistentPty.update | `persistent_pty::update` → v2: `experimental.persistentPty.update` | ⚠ 命名差异 |
| 28 | GET | `/api/experimental/persistent-pty/{ptyID}/connect` | persistentPty.connect | `persistent_pty::connect` | v2 SDK 未暴露（WS） |
| 29 | POST | `/api/experimental/persistent-pty/{ptyID}/connect-token` | server.experimental.persistentPty.connectToken | `persistent_pty::connect_token` → v2: `experimental.persistentPty.connectToken` | ⚠ 命名差异 |
| 30 | GET | `/api/experimental/persistent-pty/{ptyID}/snapshot` | server.experimental.persistentPty.snapshot | `persistent_pty::snapshot` → v2: `experimental.persistentPty.snapshot` | ⚠ 命名差异 |
| 31 | POST | `/api/experimental/session/import` | experimental.session.import | `session::import` | OK |
| 32 | GET | `/api/experimental/session/stats` | experimental.session.stats | `session::stats` | OK（实现 PARTIAL，见 §5.7） |
| 33 | GET | `/api/experimental/session/{sessionID}/export` | experimental.session.export | `session::export` | OK |
| 34 | GET | `/api/experimental/session/{sessionID}/instructions/entries` | experimental.session.instructions.entry.list | `session::instructions::entry_list` → v2: `session.instructions.entry.list` | ⚠ 命名差异 |
| 35 | DELETE | `/api/experimental/session/{sessionID}/instructions/entries/{key}` | experimental.session.instructions.entry.remove | `session::instructions::entry_remove` → v2: `session.instructions.entry.remove` | ⚠ 命名差异 |
| 36 | PUT | `/api/experimental/session/{sessionID}/instructions/entries/{key}` | experimental.session.instructions.entry.put | `session::instructions::entry_put` → v2: `session.instructions.entry.put` | ⚠ 命名差异 |
| 37 | GET | `/api/experimental/session/{sessionID}/log` | session.log | `session::log` | OK（SSE） |
| 38 | POST | `/api/experimental/session/{sessionID}/skill` | experimental.session.skill | `session::skill` | OK |
| 39 | GET | `/api/experimental/session/{sessionID}/terminal` | server.experimental.persistentPty.list | `persistent_pty::list` → v2: `experimental.persistentPty.list` | ⚠ 命名差异 |
| 40 | POST | `/api/experimental/session/{sessionID}/terminal` | server.experimental.persistentPty.create | `persistent_pty::create` → v2: `experimental.persistentPty.create` | ⚠ 命名差异 |
| 41 | GET | `/api/experimental/session/{sessionID}/terminal/read` | server.experimental.persistentPty.read | `persistent_pty::read` → v2: `experimental.persistentPty.read` | ⚠ 命名差异 |
| 42 | POST | `/api/experimental/session/{sessionID}/wait` | experimental.session.wait | `session::wait` | OK |
| 43 | GET | `/api/form` | form.list | `form::list` | OK |
| 44 | GET | `/api/fs/find` | fs.find | `fs::find` → v2: `file.find` | ⚠ 命名差异 |
| 45 | GET | `/api/fs/list` | fs.list | `fs::list` → v2: `file.list` | ⚠ 命名差异 |
| 46 | GET | `/api/fs/read/*` | fs.read | `fs::read` → v2: `file.read` | ⚠ 命名差异 |
| 47 | GET | `/api/info` | server.info | `server::info` | OK |
| 48 | GET | `/api/integration` | integration.list | `integration::list` | OK |
| 49 | GET | `/api/integration/{integrationID}` | integration.get | `integration::get` | OK |
| 50 | POST | `/api/integration/{integrationID}/connect/command` | integration.command.connect | `integration::command::connect` | OK |
| 51 | DELETE | `/api/integration/{integrationID}/connect/command/{attemptID}` | integration.command.cancel | `integration::command::cancel` | OK |
| 52 | GET | `/api/integration/{integrationID}/connect/command/{attemptID}` | integration.command.status | `integration::command::status` | OK |
| 53 | POST | `/api/integration/{integrationID}/connect/key` | integration.connect.key | `integration::connect::key` | OK |
| 54 | POST | `/api/integration/{integrationID}/connect/oauth` | integration.oauth.connect | `integration::oauth::connect` | OK |
| 55 | DELETE | `/api/integration/{integrationID}/connect/oauth/{attemptID}` | integration.oauth.cancel | `integration::oauth::cancel` | OK |
| 56 | GET | `/api/integration/{integrationID}/connect/oauth/{attemptID}` | integration.oauth.status | `integration::oauth::status` | OK |
| 57 | POST | `/api/integration/{integrationID}/connect/oauth/{attemptID}/complete` | integration.oauth.complete | `integration::oauth::complete` | OK |
| 58 | GET | `/api/location` | location.get | `location::get` | OK |
| 59 | POST | `/api/location/reload` | location.reload | `location::reload` | OK |
| 60 | GET | `/api/mcp` | mcp.list | `mcp::list` | OK |
| 61 | GET | `/api/mcp/resource` | mcp.resource.catalog | `mcp::resource::catalog` | OK |
| 62 | GET | `/api/model` | model.list | `model::list` | OK |
| 63 | GET | `/api/model/default` | model.default | `model::default` | OK |
| 64 | POST | `/api/pair` | server.pair | `server::pair` | OK |
| 65 | GET | `/api/permission/request` | permission.request.list | `permission::request::list` | OK |
| 66 | GET | `/api/permission/saved` | permission.saved.list | `permission::saved::list` | OK |
| 67 | DELETE | `/api/permission/saved/{id}` | permission.saved.remove | `permission::saved::remove` | OK |
| 68 | GET | `/api/plugin` | plugin.list | `plugin::list` | OK |
| 69 | POST | `/api/plugin/check` | plugin.check | `plugin::check` | OK（实现 STUB，见 §5.6） |
| 70 | POST | `/api/plugin/update` | plugin.update | `plugin::update` | OK（实现 STUB，见 §5.6） |
| 71 | GET | `/api/project` | project.list | `project::list` | OK |
| 72 | PATCH | `/api/project/{projectID}` | project.update | `project::update` | OK |
| 73 | GET | `/api/provider` | provider.list | `provider::list` | OK |
| 74 | GET | `/api/provider/{providerID}` | provider.get | `provider::get` | OK |
| 75 | GET | `/api/pty` | pty.list | `pty::list` | OK |
| 76 | POST | `/api/pty` | pty.create | `pty::create` | OK |
| 77 | DELETE | `/api/pty/{ptyID}` | pty.remove | `pty::remove` | OK |
| 78 | GET | `/api/pty/{ptyID}` | pty.get | `pty::get` | OK |
| 79 | PUT | `/api/pty/{ptyID}` | pty.update | `pty::update` | OK |
| 80 | GET | `/api/pty/{ptyID}/connect` | pty.connect | `pty::connect` | v2 SDK 未暴露（WS）；实现偏差见 §5.5 |
| 81 | POST | `/api/pty/{ptyID}/connect-token` | pty.connect.token | `pty::connect_token` → v2: `pty.connect.token` | ⚠ 命名差异 |
| 82 | GET | `/api/reference` | reference.list | `reference::list` | OK |
| 83 | POST | `/api/rpc/{rpcID}/{method}` | rpc.call | `rpc::call` | OK（实现 PARTIAL，见 §5.6） |
| 84 | GET | `/api/session` | session.list | `session::list` | OK |
| 85 | POST | `/api/session` | session.create | `session::create` | OK |
| 86 | GET | `/api/session/active` | session.active | `session::active` | OK |
| 87 | DELETE | `/api/session/{sessionID}` | session.remove | `session::remove` | OK |
| 88 | GET | `/api/session/{sessionID}` | session.get | `session::get` | OK |
| 89 | PATCH | `/api/session/{sessionID}` | session.update | `session::update` | OK（title="" 再生成 PARTIAL，见 §5.7） |
| 90 | POST | `/api/session/{sessionID}/agent` | session.switchAgent | `session::switch_agent` | OK |
| 91 | POST | `/api/session/{sessionID}/background` | session.background | `session::background` | OK |
| 92 | POST | `/api/session/{sessionID}/command` | session.command | `session::command` | OK |
| 93 | POST | `/api/session/{sessionID}/compact` | session.compact | `session::compact` | OK |
| 94 | GET | `/api/session/{sessionID}/context` | session.context | `session::context` | OK |
| 95 | GET | `/api/session/{sessionID}/diff` | session.diff | `session::diff` | OK |
| 96 | PUT | `/api/session/{sessionID}/environment` | session.environment | `session::environment` | OK |
| 97 | POST | `/api/session/{sessionID}/fork` | session.fork | `session::fork` | OK |
| 98 | GET | `/api/session/{sessionID}/form` | session.form.list | `session::form::list` | OK |
| 99 | POST | `/api/session/{sessionID}/form` | session.form.create | `session::form::create` | OK |
| 100 | DELETE | `/api/session/{sessionID}/form/{formID}` | session.form.cancel | `session::form::cancel` | OK |
| 101 | GET | `/api/session/{sessionID}/form/{formID}` | session.form.get | `session::form::get` | OK |
| 102 | POST | `/api/session/{sessionID}/form/{formID}/reply` | session.form.reply | `session::form::reply` | OK |
| 103 | POST | `/api/session/{sessionID}/generate` | session.generate | `session::generate` | OK |
| 104 | GET | `/api/session/{sessionID}/inbox` | session.inbox.list | `session::inbox::list` | OK |
| 105 | DELETE | `/api/session/{sessionID}/inbox/{inboxID}` | session.inbox.cancel | `session::inbox::cancel` | OK |
| 106 | PATCH | `/api/session/{sessionID}/inbox/{inboxID}` | session.inbox.update | `session::inbox::update` | OK |
| 107 | POST | `/api/session/{sessionID}/interrupt` | session.interrupt | `session::interrupt` | OK |
| 108 | GET | `/api/session/{sessionID}/message` | session.message.list | `message::list` → v2: `message.list` | ⚠ 命名差异（同） |
| 109 | GET | `/api/session/{sessionID}/message/{messageID}` | session.message.get | `message::get` → v2: `session.message.get` | ⚠ 命名差异 |
| 110 | POST | `/api/session/{sessionID}/model` | session.switchModel | `session::switch_model` | OK |
| 111 | POST | `/api/session/{sessionID}/move` | session.move | `session::move_` → v2: `session.move` | ⚠ 命名差异（`move` 是 Rust 关键字） |
| 112 | GET | `/api/session/{sessionID}/permission` | session.permission.list | `session::permission_list` → v2: `permission.list` | ⚠ 命名差异 |
| 113 | POST | `/api/session/{sessionID}/permission` | session.permission.create | `session::permission_create` → v2: `permission.create` | ⚠ 命名差异 |
| 114 | GET | `/api/session/{sessionID}/permission/{requestID}` | session.permission.get | `session::permission_get` → v2: `permission.get` | ⚠ 命名差异 |
| 115 | POST | `/api/session/{sessionID}/permission/{requestID}/reply` | session.permission.reply | `session::permission_reply` → v2: `permission.reply` | ⚠ 命名差异 |
| 116 | POST | `/api/session/{sessionID}/prompt` | session.prompt | `session::prompt` | OK |
| 117 | DELETE | `/api/session/{sessionID}/revert` | session.revert.clear | `session::revert::clear` | OK |
| 118 | POST | `/api/session/{sessionID}/revert/commit` | session.revert.commit | `session::revert::commit` | OK |
| 119 | POST | `/api/session/{sessionID}/revert/stage` | session.revert.stage | `session::revert::stage` | OK |
| 120 | POST | `/api/session/{sessionID}/shell` | session.shell | `session::shell` | OK |
| 121 | POST | `/api/session/{sessionID}/synthetic` | session.synthetic | `session::synthetic` | OK |
| 122 | POST | `/api/session/{sessionID}/view` | session.view | `session::view` | OK |
| 123 | GET | `/api/shell` | shell.list | `shell::list` | OK |
| 124 | POST | `/api/shell` | shell.create | `shell::create` | OK |
| 125 | DELETE | `/api/shell/{id}` | shell.remove | `shell::remove` | OK |
| 126 | GET | `/api/shell/{id}` | shell.get | `shell::get` | OK |
| 127 | GET | `/api/shell/{id}/output` | shell.output | `shell::output` | OK |
| 128 | GET | `/api/skill` | skill.list | `skill::list` | OK |
| 129 | GET | `/api/vcs` | vcs.get | `vcs::get` | OK |
| 130 | GET | `/api/vcs/base` | vcs.base | `vcs::base` | OK |
| 131 | GET | `/api/vcs/branch` | vcs.branch.list | `vcs::branch::list` | OK |
| 132 | GET | `/api/vcs/diff` | vcs.diff | `vcs::diff` | OK（实现 PARTIAL，见 §5.7） |
| 133 | POST | `/api/vcs/init` | vcs.init | `vcs::init` | OK |
| 134 | GET | `/api/vcs/status` | vcs.status | `vcs::status` | OK |
| 135 | POST | `/api/websearch` | websearch.query | `websearch::query` | OK |
| 136 | GET | `/api/websearch/provider` | websearch.providers | `websearch::providers` | OK |
| 137 | DELETE | `/api/worktree` | worktree.remove | `worktree::remove` | OK（带 JSON body 的 DELETE） |
| 138 | GET | `/api/worktree` | worktree.list | `worktree::list` | OK |
| 139 | POST | `/api/worktree` | worktree.create | `worktree::create` | OK |
| 140 | POST | `/api/worktree/refresh` | worktree.refresh | `worktree::refresh` | OK（实现 STUB，见 §5.7） |
| 141 | GET | `/auth/connect/{code}` | server.connect | `server::connect` | OK |

### 1.1 非协议 HTTP 面

| 面 | v2 | Rust | 结论 |
|---|---|---|---|
| `GET /openapi.json` | `routes.ts:176`（openapiPath） | `assets.rs openapi_response`，字节 sha-pin（`goldens/p10`） | parity |
| pre-ready `GET /api/info` | `process.ts:185-188,200-214`（authed，200/500/503 + `retry-after: 1`） | `app.rs:210-215` + `info.rs:21-47` | parity（body 差一个字段，见 §5.8-F13） |
| 未就绪其它路由 | `process.ts:216-230` 503 `{code}` 信封 | `info.rs:62-75` + `errors.rs:492-521` | parity |
| 静态 Web UI | CLI `serveUI`（SPA 回退、`_assets|icons` 404 no-store） | `assets.rs`/`app_archive.rs` 同语义；默认空档（R-04 KNOWN：v2.0.24 无独立产物，需源码构建内嵌） | parity（构建产物缺口 KNOWN） |
| 未匹配 API 404 / 405 | 空 body 404；Effect 路由无 405 | `app.rs:310-312`（axum 405 改写为空 404） | parity |
| 方法不匹配 | 同上 | 同上 | parity |

---

## 2. OpenAPI 生成物一致性

- ref `packages/protocol/openapi.json` 与 rust `tools/openapi-gen/openapi.json` **sha256 完全相同**；程序化 diff：paths 内容差异 0、components/schemas 差异 0、schemas 集合差 0、顶层（info/openapi/security/tags）差 0。
- rust 生成链自带不变量测试（`tools/openapi-gen/tests/gen.rs`）：确定性输出、`OPERATION_IDS` 与 `routes_gen` 141/141 对齐、pinned-spec sha、命名空间覆盖；`routes.gen.rs:11` 记录同一 sha。
- `/openapi.json` 由服务器按 pinned 字节返回（`assets.rs`，测试 `server_assets_openapi_bytes_match_pinned_sha`）。
- **结论：生成物与 v2 schema 完全一致；类型字段差异（optional/null/default/enum）因此为零**（`oc-client/src/types.gen.rs` 467 个 struct、枚举均由同一份规范生成，`enum` 处理见文件头注释：带 singleton-enum 判别式的 union 生成 Rust enum）。

---

## 3. 差异清单（EXPECTED vs ACTUAL）

严重级定义：S0 = 破坏核心互操作；S1 = 特定子系统功能损坏/安全语义偏差；S2 = 边角行为偏差或已登记桩。分类：MISSING/STUB/PARTIAL/WRONG/EXTRA。

### S1（NEW，未在任何文档登记）

| # | 分类 | 差异 | ref path:line | rust path:line | 影响 |
|---|------|------|---------------|----------------|------|
| F1 | WRONG | **普通 PTY WS meta 帧三重偏差**：(a) 顺序反了——v2 先发 replay 分块、再发 metaFrame；Rust 先发 meta 再发 replay；(b) 编码错——v2 metaFrame 是**二进制帧** `0x00 + {"cursor":n}`，Rust 是**文本帧** `{"type":"meta","cursor":n}`；(c) 分块粒度 64KiB vs 16KiB | `core/pty/protocol.ts:14-22`（metaFrame/REPLAY_CHUNK=64KiB）、`server/handlers/pty.ts:198-199`（先 replay 后 meta） | `oc-server/src/handlers/pty.rs:322-329`（meta 先行、`Message::Text`、`chunks(16*1024)`） | v2 终端客户端（web/desktop）按"replay→二进制 meta"消费：Rust 流的 meta 永远不被识别（文本帧、且在最前），**断线重连的游标恢复失效**；客户端若按二进制解析首帧会拿到非 NUL 起始的文本 |
| F2 | WRONG | **PTY 退出关闭码**：v2 `onEnd` → `CloseEvent(1000)`（正常关闭）；4404 仅用于 attach 失败（"session not found"/"session exited"，带 1s 排空窗口）。Rust：正常退出也发 **4404** `"pty exited{code}"`；attach 失败发 4404 `"attach failed"`（无 1s 窗口、无区分文案） | `server/handlers/pty.ts:169-176,186,190-194` | `oc-server/src/handlers/pty.rs:298-305`（on_end→4404）、`pty.rs:313-320`（attach 失败立即关） | 客户端把正常退出误判为"会话不存在"类异常路径；关闭原因文案丢失 |
| F3 | WRONG | **persistent-PTY `resized` 帧缺 `checkpoint`**：v2 序列化 `{...event, checkpoint: base64}`（终端快照增量），参考 TUI 用它走 `ESC c + checkpoint` 快进路径；Rust 只发 `{"type":"resized","cols","rows"}` | `server/handlers/persistent-pty.ts:141-145`、消费方 `tui/src/component/terminal-pane.tsx:20-21,114` | `oc-server/src/handlers/persistent_pty.rs:333-337` | 视口 resize 后参考客户端无法快进，只能全量重放；带宽/延迟退化（不丢数据） |
| F4 | MISSING | **WS connect 票据消费缺 Origin 白名单门**：v2 在 `ticket` 存在时先判 `isAllowedRequestOrigin(origin, host, cors)`，不通过直接 403 且**不消费**票据；Rust 两个 connect 都无条件消费（仅校验 scope） | `server/handlers/pty.ts:153-159`、`persistent-pty.ts:112-118` | `oc-server/src/handlers/pty.rs:263-267`、`persistent_pty.rs:272-278`（`origin_allowed` 只在 connect-token 用到：`pty.rs:166-168`、`persistent_pty.rs:243`） | 安全语义偏差：被禁 Origin 的页面持有效票据可完成连接（缓解：票据单次有效、60s TTL、UUID 不可猜、需先通过 connect-token 的 Origin 门拿到票据——但票据可经其它信道泄露） |

### S2（KNOWN：已在 reposcan/auditE/auditD/ROADMAP 登记）

| # | 分类 | 差异 | ref | rust | 登记 |
|---|------|------|-----|------|------|
| F5 | STUB | `POST /api/plugin/check`：v2 查 npm registry 标注 `outdated:true`、未知 target 400；Rust 忽略 `target` 恒返回空清单 | `server/handlers/plugin.ts:26-62` | `handlers/misc.rs:152-162` | KNOWN（auditE-core #75） |
| F6 | STUB | `POST /api/plugin/update`：v2 真实更新、未知 targets 400、失败 503；Rust 恒 204 | `server/handlers/plugin.ts:64-90` | `handlers/misc.rs:164-174` | KNOWN（同上） |
| F7 | PARTIAL | `POST /api/rpc/:rpcID/:method`：v2 经 `Rpc.Service` 分发进插件宿主（RpcError 400 / RpcInternalError 500 映射）；Rust 恒 `rpc.internal` 500（`NoopRpcPort`） | `server/handlers/rpc.ts:5-27`、`core/rpc.ts` | `handlers/misc.rs:180-188` | KNOWN（reposcan §2.3/§3） |
| F8 | STUB | `POST /api/worktree/refresh`：v2 重扫 git worktree 入表；Rust 纯 no-op 204 | `server/handlers/worktree.ts:21-23` | `handlers/project.rs:557-561` | KNOWN（phase-9，reposcan #220） |
| F9 | PARTIAL | `GET /api/experimental/session/stats`：v2 有按日/按模型的分桶活动与工具可靠性；Rust 仅会话计数 + token/cost 总计 | `protocol/groups/session.ts:200-218`（SessionStats.Info） | `handlers/session.rs:1789-1795` | KNOWN（phase-9） |
| F10 | PARTIAL | `GET /api/vcs/diff`：v2 返回文件级 `FileDiffInfo[]`；Rust 把原始 unified diff 塞进单条合成条目 `{path:"diff",status:modified,chunks:[raw]}` | `server/handlers/vcs.ts`（diff 处理） | `handlers/project.rs:680-699` | KNOWN（phase-9 / file_diff lane） |
| F11 | PARTIAL | `PATCH /api/session/:id` `title:""`（LLM 再生成标题）未实现（仅非空改名） | `server/handlers/session.ts:360+` | `handlers/session.rs:466-475`（TODO(phase-9)） | KNOWN（reposcan #224） |
| F12 | EXTRA | 公共 SSE 流多出 3 个 v2 不公开的类型：`mcp.prompts.changed`（KNOWN）、`mcp.tools.changed`（auditD 误标为 parity——`event-manifest.ts:79-80` 只含 status/resources 两个 Mcp 事件）、`session.diff`（v1 遗留，v2 公共集不含） | `schema/event-manifest.ts:71-81` | `sse.rs:164-169` + `oc-core/bus/registry.rs:380-383`（tools/prompts）、`registry.rs:249-253`（session.diff） | 部分 KNOWN（prompts：auditD #10；tools 与 session.diff 为本次**细化/新确认**） |

### S2（NEW：本次新发现）

| # | 分类 | 差异 | ref path:line | rust path:line | 影响 |
|---|------|------|---------------|----------------|------|
| F13 | EXTRA | 浏览器过期配对链接的 401 文本响应多带 `www-authenticate: Basic …` 头；v2 纯文本 401 无挑战头（避免浏览器弹原生认证框）。API 分支（信封 401）两侧一致 | `server/handlers/server.ts:38-42` | `oc-server/src/pairing.rs:82-93`（:91 插入挑战头） | 浏览器访问过期 `/auth/connect/*` 会弹原生 Basic 认证框而非展示提示文本 |
| F14 | EXTRA | pre-ready `/api/info`（503/500 body）多 `capabilities` 字段：v2 的 `process.ts infoResponse` 只回 `{version,pid,urls,paths}`；就绪后的 handler 版本两侧都含 capabilities | `server/process.ts:200-214` vs `handlers/server.ts:17-27` | `oc-server/src/info.rs:29-38`（恒含 capabilities） | 字段级多余；宽松客户端可解码，严格 golden 比对会差一字段 |
| F15 | PARTIAL | `/api/info` `urls` 不展开通配绑定：v2 `connectionURLs` 把 `0.0.0.0`/`::` 展开为各非内部网卡地址（hostname 可配置，路径可达）；Rust 恒返回绑定形 `http://0.0.0.0:port` | `server/server-info.ts:18-36`、`process.ts:58-63` | `oc-server/src/info.rs:49-57`（注释自认折叠） | 通配绑定时客户端拿到的 url 不可直接连接 |
| F16 | WRONG | persistent-pty `attached` 帧的 `role` 取**请求参数**；v2 取 **attachment.role**（服务端裁定，如 observer 接管后可能与请求不同） | `server/handlers/persistent-pty.ts:169`（`role: attachment.role`） | `oc-server/src/handlers/persistent_pty.rs:382-385` | 角色降级/接管时客户端看到的角色可能与实际不符 |
| F17 | WRONG | framed-input 解码过松：v2 仅接受 type∈{0,1} 且 cols/rows 非 0，否则丢弃；Rust 任意 type（0=控制、其余=输入）且不查 0 值 | `server/handlers/persistent-pty.ts:213-220` | `oc-server/src/handlers/persistent_pty.rs:477-487` | 非法帧被写入 PTY（垃圾输入/0 尺寸 resize） |
| F18 | WRONG | 入站二进制帧的无效 UTF-8：v2 fatal 解码失败**整帧丢弃**；Rust `from_utf8_lossy` 写入 U+FFFD | `core/pty/protocol.ts:35-41`（decodeInput） | `oc-server/src/handlers/pty.rs:350-351`、`persistent_pty.rs:454` | 终端收到替换字符垃圾而非静默丢帧 |
| F19 | PARTIAL | `PUT /api/experimental/persistent-pty/:id` resize 丢 `attachmentID` 归因参数（v2 `resize(id, cols, rows, attachmentID)`） | `server/handlers/persistent-pty.ts:73-81` | `oc-server/src/handlers/persistent_pty.rs:178-181`（传 `None`） | resize 不归属到 attachment（多端场景下旁观者触发 resize 归因丢失） |
| F20 | EXTRA | Rust 侧 `activate()` 在排队 replay/meta **之前**调用（v2 先排队后激活），激活回调竞态下 live 输出可能插到 replay 之前 | `server/handlers/pty.ts:198-201` | `oc-server/src/handlers/pty.rs:308-329` | 窗口极小的帧序竞态（persistent-pty 侧顺序正确：`persistent_pty.rs:394-401`） |
| F21 | EXTRA | client 扩展 `event().subscribe_from(last_event_id)` 发送 `Last-Event-ID` 头——v2 无此客户端 API，且**两侧服务器都不支持** SSE 重放（纯 advisory） | （无；v2 仅 `client/test/solid-connection.test.ts` 提及） | `oc-client/src/event_ext.rs:14-39` | 良性扩展：服务器忽略该头，行为与 v2 一致 |
| F22 | WRONG(元数据) | `routes.gen.rs` 把 `GET /api/worktree` 的 `projectID` 标为 `required:false`；OpenAPI 实为 `required:true`（处理器按必填校验，行为正确） | `protocol/openapi.json` `/api/worktree.get.parameters` | `oc-server/src/routes.gen.rs:2218` | 生成表元数据不准（仅文档/测试断言风险） |

### Footnote（不构成缺陷，记录备查）

1. **JSON 键序**：rust 侧 `serde_json::Map`（BTreeMap）按键排序，v2 按插入序；语义等价，仅逐字节 golden 比对需注意（rust 侧 golden 已按自身序 pin）。
2. **压缩层跳过 SSE**：`app.rs:34-44` 显式排除 `text/event-stream`；v2 用 effect 通用压缩中间件。属安全侧加固（SSE 过压缩会粘帧），非回归。
3. **`x-opencode-build` 响应头**：rust 在 `/api/info` 附加（B4-H3 特性，KNOWN 且有意为之——头不在 body/openapi 中，对 v2 客户端不可见）。
4. **404 内容协商**：v2 未匹配路由空 404；rust 同（`errors::empty`）。405→404 改写已对齐（`app.rs:310-312`）。

---

## 4. 客户端 / SDK 方法覆盖与命名

### 4.1 覆盖

- v2 生成客户端（`packages/client/src/promise/generated/client.ts`）暴露 **139** 个方法（30 个顶层命名空间）；两个 WS upgrade 操作（`pty.connect`、`persistentPty.connect`）**不**生成方法（由 `client/src/solid/pty.ts` 专用 WS 客户端处理，经 `connect-token` 换票后建 WS）。
- rust `oc-client/src/methods.gen.rs` 暴露 **141** 个方法（含上述 2 个 WS 操作按 OpenAPI 的布尔 GET 建模，`TODO(pty-ws)` 登记）。
- **线上契约 1:1**（同一份 pinned OpenAPI 生成；`OPERATION_IDS` 断言）。

### 4.2 命名差异（29 个方法，纯 Rust 命名法，不影响线上）

| Rust | v2 | 说明 |
|---|---|---|
| `fs::*`（4 个） | `file.*` | 命名空间名不同 |
| `persistent_pty::*`（10 个） | `experimental.persistentPty.*` | 命名空间不同（flat vs 二级） |
| `pty::connect_token` | `pty.connect.token` | flat |
| `migration::status` | `migration.v1.status` | 丢 `v1` 段 |
| `integration::wellknown_add` | `integration.wellknown.add` | flat |
| `session::permission_{list,create,get,reply}` | `permission.{list,create,get,reply}` | 提升到 permission 命名空间 |
| `session::instructions::entry_{list,put,remove}` | `session.instructions.entry.{list,put,remove}` | 尾段 flat |
| `message::{list,get}` | `message.list` / `session.message.get` | get 的归属不同 |
| `session::move_` | `session.move` | Rust 关键字避让 |
| `session::switch_agent` / `switch_model` 等 | `switchAgent` / `switchModel` | snake_case（通用差异） |

### 4.3 SDK 层（`packages/sdk`）架构差异

- v2 `@opencode/sdk` 的 `create()`（`sdk/src/promise.ts:33-59`）= **内嵌宿主**：按需拉起进程内 server、插件注册（`PluginPromise`）、多实例配置（`instances.configure`）、`Interface = Omit<OpenCodeClient,"plugin"> & …`。
- rust `oc-client` 是纯 HTTP/SSE 客户端；内嵌宿主职能由 `oc-cli` 的服务管理（`lifecycle.rs` 的 ensure/stop/contender/pty-handoff 全套）承担。**SDK 级别的"库内嵌服务器"形态缺失**——这是有意的架构分工，非路由/协议缺口，列为 SDK 形态差异（MISSING-at-SDK-level，等价能力在 CLI 层实现且时序常量逐项对齐：probe 2s、stop 50ms×100、watcher 5s、EADDRINUSE 100ms×15s、ensure 120s，`lifecycle.rs:8-9`）。
- v2 SDK 的 effect 变体（`sdk/src/effect/*`）无 rust 对应（Rust 生态无 Effect；`oc-client` 即 promise 形态）。

---

## 5. 分节核对

### 5.1 认证（Basic / auth_token / cookie / pairing ticket）

逐项 parity：固定用户名 `opencode`（`auth.rs:30` ≡ `auth.ts:20`）；Basic 大小写不敏感 scheme、首冒号切分、坏 base64/无冒号→空凭据（`auth.rs:72-100` ≡ `authorization.ts:18-38`）；padding 容忍解码（`auth.rs:73-74`，比 v2 Node 语义更精确）；`auth_token` query 优先于头（`auth.rs:259-266`）；会话令牌 `${expires}.${hmac}`、KDF info `opencode-session-v1`、30d TTL、常数时间比较、密码轮换吊销（`auth.rs:120-172` ≡ `auth.ts:40-66`）；cookie 名带端口（IPv6 括号感知，`auth.rs:177-208`）+ Origin 同源限制（`auth.rs:269-282`）；401 信封 `{_tag:UnauthorizedError,message:"Authentication required"}` + `www-authenticate` 仅当 `sec-fetch-mode` 缺失或 `navigate`（`auth.rs:287-305` ≡ `authorization.ts:40-55`）；pairing 5min/1000 容量/单次消费、浏览器 302+Set-Cookie(HttpOnly/Lax/Max-Age)/API `{token}`、过期文案逐字（`pairing.rs` ≡ `pairing.ts`+`handlers/server.ts`）——唯一偏差 F13（浏览器 401 多挑战头）。PTY connect-token：`x-opencode-ticket: 1` 头门 + Origin 门（`pty.rs:163-168` ≡ `pty.ts:122-127`）；票据 60s/10k/UUID/scope 精确匹配单次消费、scope 不符不消费（`tickets.rs:52-64` ≡ `core/pty/ticket.ts`）。

### 5.2 CORS / middleware / 请求追踪

- 白名单逐字：`http://localhost:*`、`http://127.0.0.1:*`、`oc://renderer`、三个 tauri host、`https://(*.)opencode.ai`（手写无正则依赖，含大小写与多标签边界，`middleware.rs:27-76` ≡ `cors.ts:3-20`）；`isAllowedRequestOrigin` 同 host 优先（`middleware.rs:81-91`）。
- 预检 204 + 回显 method/headers + `max-age: 86400` + `Vary: Origin`；实际请求放行仅加 `access-control-allow-origin`；不允许的 origin 不阻断只无头（`middleware.rs:111-180` ≡ effect `HttpMiddleware.cors({allowedOrigins,maxAge:86400})`，`process.ts:71`）。层序：CORS 最外 → 压缩 → dispatch（`app.rs:178-186` ≡ `process.ts:69-74` pipe 序）。
- Location 推导：`location[directory]` query → `x-opencode-directory` 头（URI 解码，坏序列透传）→ cwd（`middleware.rs:249-269` ≡ `location.ts:57-65`）；目录不存在 → 404 `LocationNotFoundError`（`common.rs:249-261`）。
- 请求追踪：v2 `request-tracing.ts` 仅 `withoutParentSpan`（切断启动 span 继承，4 行）；rust 无 span 概念——无对应物，视为环境差异（无功能影响）。v2 的 `errorResponseLogger`（仅 ≥400 记日志）在 rust 侧对应 `ApiError::into_response` 的 5xx 恒落盘 + 4xx 不落（`errors.rs:42-53,580-585`，测试覆盖）。

### 5.3 Server lifecycle / readiness / shutdown

- 状态机 `starting→ready|failed`（仅自 starting）、`stopping` 幂等且压倒一切（`readiness.rs` ≡ `service-status.ts:18-30`）。
- 绑定：默认 `127.0.0.1`；未指定端口时自 4096 向上扫描（EADDRINUSE），CLI 路径带 100ms×15s 在位者轮询（`lifecycle.rs:1399-1417,1438-1468` ≡ `process.ts:139-168`）。
- 停机：v2 finalizer = `beginStopping` → 卸载 app → `closeAllConnections`；rust = readiness→Stopping → shutdown watch → `with_graceful_shutdown` 排空，且**所有无限流体（SSE 事件流、session-log follow、两个 PTY WS 循环）select 在 shutdown 上**（gate-6 F2）——比 v2 更完备（v2 `pty.ts:180` 自留 TODO "Integrate graceful-shutdown socket tracking"）。
- 注册文件/probe/contender/handoff：`lifecycle.rs` 全套（filename per channel、原子写、私有权限、stale/回收 PID/外来进程防护、`x-opencode-build` 新旧判定），R-09 已 done（daemon 进程边界为登记的 phase-9 余项）。

### 5.4 SSE（/api/event 与 session log）

- 帧格式 `data: {json}\n\n`（`sse.rs:33-35` ≡ `event-feed.ts:29-31`）；响应头四件套逐字（`sse.rs:338-358` ≡ `handlers/event.ts:22-32`）；首帧合成 `server.connected {id:"evt_…",data:{}}`（`sse.rs:362-365` ≡ `event.ts:13-17`）；15s `: heartbeat\n\n` 且随主流终止（`sse.rs:29,452-458` ≡ `event.ts:21-23` haltStrategy:left）。
- 背压语义：4096 dropping 队列满 → 订阅者移除并以 `SubscriberOverflow` 终止；编码失败 → 全体订阅者断开、feed 存活（`sse.rs:87-116,210-234`，测试 ≡ `event-feed.ts:48-69`）。
- 事件集合：BE-1 之后缺失清零（原 20 个缺失类型已全部入注册表，`registry.rs:386-411` 注释即 auditD §3.4 清单）；多余 3 个见 F12。`rpc.*` 前缀放行两侧一致（`groups/event.ts:66-68` ≡ `sse.rs:167`）。
- `session log`：`after`(NumberFromString)/`follow`(BooleanFromString) 校验、会话存在性 404、`data:` 帧 + 单个 `log.synced {type,aggregateID,seq?}` 水位标记（`sse.rs:493-577`、`oc-schema events.rs LogSynced` ≡ `handlers/session.ts:610-618`、`schema/event-log.ts:11-21`）。
- 事件 id/重放：**两侧服务器均不发 SSE `id:` 行、均不实现 Last-Event-ID 重放**（v2 全仓 grep 仅一个测试提及）；断线补齐靠各客户端自持游标。Rust client 的 `Last-Event-ID` 扩展为 advisory（F21）。

### 5.5 WebSocket / PTY upgrade

- 普通 PTY（`/api/pty/:id/connect`）：404 前置、票据门、`cursor`（safe-int ≥ -1）、升级后 replay→meta→live、`decodeInput` 文本透传/二进制 UTF-8。Rust 的会话生命周期、shutdown 结束、输入互斥均落地，但存在 F1/F2/F18/F20/F4 五项偏差（见 §3）。
- persistent PTY：`cursor`(≥0, 非法 400)、`role`、`input_protocol`、`attachment_id`（缺省 UUID）、`takeover` 全支持；`attached`（含 replay 三元组 + truncated）→ 二进制 replay → `replay_complete` 顺序正确；`exited` 帧、`onEnd`→Close 1000、attach 失败→4404 `"terminal unavailable"` 均对齐（`persistent_pty.rs:260-401` ≡ `persistent-pty.ts:108-185`）。偏差：F3（checkpoint）、F16（role 来源）、F17（framed 校验）、F4（Origin 门）。
- v2 的 solid PTY 客户端先 `connect-token`（`x-opencode-ticket:1`）换票再建 WS（`client/src/solid/pty.ts:26-33,49-66`）——rust 处理器对这条链路的服务端语义兼容（除上列偏差）。

### 5.6 插件与 RPC 面

`plugin.list` 真实清单（wave-12 子系统，含 active/failed/crash-disabled 状态；登记的偏差：进程级全局 vs v2 每 location 激活）；`plugin.check`/`plugin.update` 桩（F5/F6）；`/api/rpc` 恒 500（F7）。`oc-server/plugins.rs` 的会话插件 RPC 缝（prompt/get/list/messages/interrupt，更宽面显式报错）为登记的 phase-9 边界。

### 5.7 已登记 phase-9 桩（行为面）

F8（worktree.refresh）、F9（stats 分桶）、F10（vcs.diff 文件级）、F11（title 再生成）。另：`session_move` 工具未接线（API 的 `session.move` 已通）为 reposcan 登记项，不在本报告路由面重复计。

### 5.8 本次新确认的细粒度项

F13（配对 401 挑战头）、F14（pre-ready info capabilities）、F15（通配绑定 urls）、F16-F20、F21、F22。

---

## 6. 客户端子系统（oc-client）核对

| 项 | v2（generated client） | rust（oc-client） | 结论 |
|---|---|---|---|
| URL 构建 | baseUrl 补尾 `/` + 相对 join（`client.ts:302-307`） | base 去尾 `/` 字符串拼接 + `render_path`（`transport.rs:61-88`） | 等价 |
| 路径参数编码 | `encodeURIComponent` 逐段、`*` 通配保斜杠 | `encode_segment`（RFC3986 unreserved + `!~*'()`）/ `encode_path`（`transport.rs:40-59`） | 等价 |
| 状态分类 | success 严格等值；declared → `declared()`；其余 `UnexpectedStatus`（`client.ts:340-360`） | 同（`transport.rs:151-165`） | parity |
| 错误模型 | `ClientError{reason…}` + declared 展开字段、`_tag` 为名、`message ?? data.message` | `ClientError::Api{status,name,message,fields}` 同语义（`error.rs:52-94`） | parity |
| SSE 解码 | 16MiB cap、CRLF 归一、`data:` 多行 join、注释跳过、JSON.parse 失败 Malformed、onActivity 每 chunk（`client.ts:362-418`） | 同（`sse.rs:12,69-185`；double-decode 为容错分支，两侧线上均为对象 JSON） | parity |
| content-type 门 | `text/event-stream` 严格、否则 `UnsupportedContentType` | 同（`transport.rs:95-102,249-257`） | parity |
| 认证注入 | 调用方 headers 合并；WS URL 走 `?auth_token=` | `with_basic_auth` 恒注头 + `url_with_auth_token`（`lib.rs:47-118`） | parity（形态略异：v2 由调用方传，rust 构造器级） |
| WS 客户端 | `solid/pty.ts`（专用） | 无 crate 内 WS（`TODO(pty-ws)` 登记） | KNOWN 登记 |

---

## 7. 覆盖清单（审计触达）

**ref 侧全文**：`server/src/{routes,api,handlers,process,auth,pairing,cors,event-feed,server-info,service-status,request-tracing,location,options,fetch}.ts`、`server/src/middleware/{authorization,schema-error,form-location,session-location}.ts`、`server/src/handlers/{server,event,session,session-error,pty,pty-socket,persistent-pty,plugin,rpc,worktree,fs,form,permission,debug,generate,integration,mcp,message,model,provider,credential,project,reference,shell,skill,vcs,websearch,config,command,agent,migration,location}.ts`、`protocol/src/{api,client,errors,simulation}.ts`、`protocol/src/groups/*.ts`（事件组重点 event.ts）、`protocol/openapi.json`（全量 diff）、`schema/src/{event,event-manifest,durable-event-manifest,event-log,location,worktree,tui-event,session-status-event,installation-event,vcs-event,mcp-event,各域 event 定义}.ts`（事件类型集合程序化提取）、`core/src/pty/{protocol,ticket}.ts`、`client/src/promise/{client,generated/client,generated/client-error}.ts`、`client/src/{shared-events,solid/pty}.ts`、`sdk/src/{opencode,promise}.ts`、TUI 消费方 `tui/src/component/terminal-pane.tsx`。
**rust 侧全文/重点**：`oc-server/src/{app,routes.gen,sse,auth,pairing,info,readiness,middleware,errors,state,lifecycle,announce,tickets,assets,app_archive,boot,plugins,safekill}.rs`、`oc-server/src/handlers/*.rs`（mod/agent/config/credential/form/fs/integration/location/mcp/message/misc/model/permission/persistent_pty/project/pty/pty_state/schema_check/server/session/shell/attachment/common）、`oc-client/src/{lib,methods.gen,types.gen,transport,sse,event_ext,error}.rs`、`oc-core/src/bus/{mod,registry,routing,sink}.rs`、`oc-schema/src/events.rs`（LogSynced 等）、`tools/openapi-gen/{openapi.json,src/*,tests/gen.rs}`、`Cargo.toml`/`Cargo.lock`（axum 0.7.9 路由语法确认）。
**文档对照**：`docs/auditD-api.md`、`docs/reposcan-findings.md`、`docs/auditE-core.md`、`ROADMAP.md`、`FINDINGS.md`（插件钩子，无 HTTP 面条目）、`docs/auditB-surfaces.md`（未含 API 细目）。

## 8. 数字汇总

| 分类 | 数量 |
|---|---|
| 路由：缺失 / 多余 | 0 / 0 |
| OpenAPI schema 字段差异 | 0（文档逐字节一致） |
| S1（NEW） | 4 条（F1-F4，PTY WS 族） |
| S2 NEW | 10 条（F13-F22） |
| S2 KNOWN（桩/部分） | 8 条（F5-F12） |
| SSE 公共事件：缺失 / 多余 | 0 / 3 |
| SDK 方法：缺失 / 多余 / 命名差异 | 0 / 2（WS 布尔法，登记）/ 29（纯命名） |
| 修复建议优先序 | F1+F2（同一文件一处修）→ F4（两处加 Origin 门）→ F3（checkpoint 透传）→ F16/F17/F18 → F13/F14（一字段一头）→ F15 → phase-9 桩按 ROADMAP 排期 |
