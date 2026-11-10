# L12 HTTP Server API — 审查报告

> 审查方式：ref server/src 全部 53 文件全量阅读 + 路由真正定义处 protocol/groups/*.ts（31 组 141 端点）提取；Rust oc-server 全部 48 文件关键面 + routes.gen.rs 142 op 全表。路由全集脚本化 diff。未使用 V2_HTTP_API_AUDIT.md 作结论来源。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明
- **Ref 全量阅读**：`packages/server/src/` 全部 53 个文件（api.ts、routes.ts、handlers.ts、handlers/* 30 个、middleware/* 4 个、auth/cors/event-feed/fetch/location/options/pairing/process/pty-environment/request-tracing/server-info/service-status/workerd.ts），以及 `packages/protocol/src/api.ts`、`groups/*.ts`（31 个，提取出全部 141 个端点）、`middleware/{authorization,schema-error}.ts`、`errors.ts`。
- **Rust 全量阅读**：`oc-server/src/` 全部 48 文件关键面：routes.gen.rs（142 op 全表提取+抽样细读）、app.rs、auth.rs（1-569 全读）、pairing.rs、tickets.rs、middleware.rs、sse.rs、errors.rs、request_validation.rs、info.rs、readiness.rs、handlers/mod.rs、pty.rs、session.rs/form.rs/misc.rs 路由挂载清单、assets/lifecycle/safekill/observability 头部。
- **路由全集 diff**：ref 141 op ↔ rust 142 op，命名差异逐一人工映射确认。

### 2. 逐项对比表

| 功能点/路由 | Ref 实现 (file:line) | Rust 实现 (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| `GET /api/agent`, `GET /api/agent/:agentID` | groups/agent.ts:10,24 | routes.gen.rs:69-94; handlers/agent.rs | ✅ | |
| `GET /api/command` | groups/command.ts:9 | routes.gen.rs:97-107; agent.rs | ✅ | |
| `GET /api/config`, `GET /api/config/shell`, `PATCH /api/experimental/config` | groups/config.ts:9,24,35 | routes.gen.rs:109-133,239-249; config.rs | ✅ | |
| credential 5 条 | groups/credential.ts:8-61 | routes.gen.rs:135-198; credential.rs | ✅ | 204/409 语义一致 |
| `GET/DELETE /api/debug/location` | groups/debug.ts:8,19 | routes.gen.rs:200-224; location.rs | ✅ | |
| `GET /api/event`（SSE） | groups/event.ts:44; handlers/event.ts:11-34 | app.rs:249-251; sse.rs:458-468 | ✅ | 见 SSE 行 |
| `POST /api/experimental/fs/write` | groups/fs.ts:76 | routes.gen.rs; fs.rs | ✅ | |
| `GET /api/fs/read/*path`, `GET /api/fs/list`, `GET /api/fs/find` | groups/fs.ts:32,47,62 | routes.gen.rs; fs.rs | ✅ | 通配参数 `*path` 一致 |
| `POST /api/experimental/generate` | groups/generate.ts:8 | routes.gen.rs; misc.rs | ✅ | |
| integration 11 条 | groups/integration.ts:16-192 | routes.gen.rs; integration.rs | ✅ | |
| `GET /api/location`, `POST /api/location/reload` | groups/location.ts:36,51 | routes.gen.rs; location.rs | ✅ | |
| mcp 6 条 | groups/mcp.ts:10-90 | routes.gen.rs; mcp.rs | ✅ | |
| `GET /api/session/:sessionID/message(:messageID)` | groups/message.ts:68 | routes.gen.rs; message.rs | ✅ | |
| `GET /api/experimental/migration/v1` | groups/migration.ts:18 | routes.gen.rs; misc.rs | ✅ | |
| `GET /api/model`, `GET /api/model/default` | groups/model.ts:10,26 | routes.gen.rs; model.rs | ✅ | |
| permission 7 条 | groups/permission.ts:23-117 | routes.gen.rs; permission.rs | ✅ | |
| persistent-pty 11 条 | groups/persistent-pty.ts:23-103 | routes.gen.rs; persistent_pty.rs | ✅ | |
| plugin list/check/update | groups/plugin.ts:10,24,40 | routes.gen.rs; misc.rs | ✅ | |
| `POST /api/plugin/install` | **无** | misc.rs:25,314-377 | 🔵 | 移植版新增（TUI 安装流 G-29） |
| project 2 条 | groups/project.ts:11,22 | routes.gen.rs; project.rs | ✅ | |
| provider 2 条 | groups/provider.ts:10,25 | routes.gen.rs; model.rs | ✅ | |
| pty 7 条（list/create/get/update/remove/connect-token/connect[WS]） | groups/pty.ts:23-120 | routes.gen.rs; pty.rs | ✅ | WS 4404/1000 关闭码、replay+metaFrame cursor、x-opencode-ticket:1 均一致（pty.rs:167-177,284-343） |
| `GET /api/reference` | groups/reference.ts:24 | routes.gen.rs; agent.rs | ✅ | |
| `POST /api/rpc/:rpcID/:method` | groups/rpc.ts:14 | routes.gen.rs; misc.rs | ✅ | |
| `GET /api/info`, `POST /api/pair`, `GET /auth/connect/:code` | groups/server.ts:42,53,64 | app.rs:243-259; info.rs:23-54; pairing.rs:66-117 | ✅ | 302+cookie/`{token}`/双 401 文案逐字一致 |
| session 家族 37 条 | groups/session.ts:180-886 | routes.gen.rs; session.rs:40-85(37 route)+form.rs | ✅ | prompt 已是 `{data: Inbox.User}` 非 SSE，两侧一致 |
| shell 5 条 | groups/shell.ts:10-72 | routes.gen.rs; shell.rs | ✅ | |
| `GET /api/skill` | groups/skill.ts:9 | routes.gen.rs; agent.rs | ✅ | |
| vcs 6 条 | groups/vcs.ts:36-110 | routes.gen.rs; project.rs | ✅ | |
| websearch 2 条 | groups/websearch.ts:10,25 | routes.gen.rs; misc.rs | ✅ | |
| worktree 4 条 | groups/worktree.ts:23-65 | routes.gen.rs; project.rs | ✅ | |
| **路由总数** | 141 ops | 142 ops | ✅ | diff 后全对位；仅 plugin.install 为 rust 新增 |
| 未匹配 API 路由 → 空 404（含方法错配不返回 405） | effect HttpApi 默认 | app.rs:310-316 显式把 405 改写为空 404 | ✅ | |
| `GET /openapi.json` | routes.ts:176 | app.rs:252-254; assets.rs:22-26 | ✅ | sha 钉住的同一文档 |
| **SSE /api/event**：帧格式、首帧合成 server.connected、15s 心跳、容量 4096、溢出断开、headers | event-feed.ts:29,8,64-68 | sse.rs:32,25,28,86-115,204-228,332-352 | ✅ | rust 额外 shutdown 时结束流体（🔵增强） |
| SSE /api/experimental/session/:id/log | handlers/session.ts:724 | sse.rs:487-580 | ✅ | |
| **认证**：Basic(user=opencode)+密码 或 会话令牌（HMAC-SHA256、base64url、常量时间比较、30 天 TTL、改密吊销全部） | auth.ts:16-66 | auth.rs:30-172 | ✅ | JS Number.isSafeInteger 语义用 f64 复刻 |
| auth_token query 凭证源 | middleware/authorization.ts:31-38 | auth.rs:259-266 | ✅ | |
| Cookie `opencode_session_{port}`（仅 Origin 缺失或同 host） | auth.ts:58-61 | auth.rs:177-282 | ✅ | |
| 401 挑战条件（sec-fetch-mode） | authorization.ts:44-55 | auth.rs:287-305 | ✅ | |
| 401 体 `{"_tag":"UnauthorizedError",...}` | authorization.ts:50-54 | errors.rs:98-99; auth.rs:287 | ✅ | |
| 认证豁免（auth/connect、pty connect+ticket ready） | authorization.ts:80-84 | middleware.rs:186-240; app.rs:216-221 | ✅ | |
| **pairing**：128bit base64url 码、TTL 300s、容量 1000、单次消费；connect 浏览器(302+cookie) vs API(`{token}`) | pairing.ts:7-34 | pairing.rs:19-117 | 🟡 | 容量满时 ref 按 LRU 淘汰，rust 淘汰"最近到期"项——极端情况下存活码集合不同，无安全影响 |
| **PTY ticket**：randomUUID、60s TTL、10k 容量、scope 精确匹配 | core/pty/ticket.ts | tickets.rs:19-64 | ✅ | |
| **CORS**：allowlist（localhost/127.0.0.1/oc://renderer/tauri/`https://*.opencode.ai`/配置项）、preflight 204+echo、max-age 86400 | cors.ts:3-26 | middleware.rs:27-180 | ✅ | rust 预检未显式回 `Vary: Access-Control-Request-Method/Headers`（有 `Vary: Origin`）；低风险 |
| **错误体**：`{"_tag":…}` + camelCase，40 余个 tagged error | protocol/errors.ts:5-267 | errors.rs:98-627 | ✅ | 40+ 构造器逐一对应；readiness 503 用 `code` 而非 `_tag`（两侧各自一致） |
| schema 校验错误 → 400 InvalidRequestError | protocol/middleware/schema-error.ts | request_validation.rs; validation.gen.rs | ✅ | 复刻了"session 路由 404 先于 payload 校验"的顺序 |
| **readiness/启动门**：pre-ready 仅放行 authed GET /api/info | process.ts:179-230 | app.rs:206-225; info.rs:23-155; readiness.rs | ✅ | rust info 额外附 `x-opencode-build` 头（🔵） |
| readiness 状态机 | service-status.ts:18-30 | readiness.rs:13-42 | ✅ | |
| **进程/监听**：hostname 默认 127.0.0.1、无 port 时 4096 起递增扫描、closeAllConnections、compression | process.ts:53,139-146,86-91,70 | lifecycle.rs:1419-1448; app.rs:180-186 | ✅ | rust 压缩排除 text/event-stream（防 SSE 帧缓冲——行为差异但更安全） |
| **location 派生**：query → x-opencode-directory 头(URI 解码) → cwd | location.ts:57-73 | middleware.rs:260-286; app.rs:283-296 | ✅ | |
| sessionID="global" form 逃生舱 | middleware/form-location.ts:27-33 | handlers/form.rs:32,240 | ✅ | |
| 会话定位中间件（404 先行） | middleware/session-location.ts:16-30 | handlers/common.rs:267+ | ✅ | |
| panic → 500 包络 | —（effect runtime） | handlers/mod.rs:105-131 | 🔵 | 显式 catch_unwind |
| 5xx 落日志 | ref 每失败打日志 | errors.rs:42-55,664-731 | ✅ | |
| 静态资源/前端 assets | 不在 ref server 包 | assets.rs; app_archive.rs（511 行） | 🔵 | rust 在 auth 前服务 app shell |
| OTel 每请求 span | ref util/observability.ts | observability.rs（otel feature 门控） | ✅ | |
| **workerd / ServerFetch**（web 标准 fetch 入口、DO 档案替换、嵌入式免监听） | workerd.ts:54-139; fetch.ts:41-54 | 无对应（grep 全 crates） | 🔴 | 移植版只有本地 hyper/axum 监听式服务 |
| safekill（PID 世代绑定、pidfd 信号） | ref 无此文件 | safekill.rs | 🔵 | 生产事故加固，新增 |

### 3. 关键差异与风险 Top 5
1. **🔴 嵌入式 fetch/workerd 入口缺失**：ref `fetch.ts`/`workerd.ts` 提供免监听 `(Request)→Response` 入口与 Cloudflare DO 运行时替换图；rust 仅 hyper 监听。若产品需要 DO/嵌入式部署则为功能缺口。
2. **🟡 pairing/ticket 容量淘汰策略差异**：ref LRU 语义，rust 淘汰最近到期项。满容量时存活码集合不同；攻击面下等价，但非严格等价。
3. **⚠️ CORS preflight 细节未逐字节证明**：effect 库 cors 的完整头集（如 `Vary: Access-Control-Request-Method`、allow-credentials）未在 ref 侧源码内展开，存在低概率头差异影响浏览器预检缓存。
4. **⚠️ 压缩层差异**：rust 排除 `text/event-stream` 压缩，ref 对整个 app 套 compression。SSE 不会错，但其它流式响应压缩行为可能不同。
5. **🔵 `POST /api/plugin/install` 为移植版新增路由**：非 ref v2.0.24 API 面；SDK/OpenAPI 消费方若以 ref 为准会视为多余端点。

### 4. 缺失文件/功能汇总
- **缺失**：workerd 运行时档案（ref workerd.ts）与 ServerFetch web-handler 入口（ref fetch.ts）。
- **新增**：plugin/install 路由；静态 assets/app_archive 服务；safekill 信号安全；panic→500 兜底；`x-opencode-build` 响应头；SSE/日志流 shutdown 主动收尾。
- 其余（路由矩阵 141 op、认证/配对/票据、SSE、CORS、错误包络、readiness、校验）均为 ✅ 或标注的 🟡/⚠️ 项。
