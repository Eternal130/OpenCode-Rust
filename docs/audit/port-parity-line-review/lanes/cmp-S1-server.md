# S1 结论: 端点面 141/141 一比一对齐（rust 仅多一个自加的 POST /api/plugin/install），传输层/认证/配对/SSE/PTY WS 移植高度忠实，主要缺口是 workerd/fetch 运行时剖面缺失、simulation 显式拒绝、vcs.init 插件 provider 不可达、shell DELETE 幂等语义三处分歧。

## 端点全量对照表

方法：ref 清单从 `protocol/src/groups/*.ts` 141 个 `HttpApiEndpoint.*` 机械提取；rust 从 `routes.gen.rs` 的 `method:/path:` 对提取（142 对）。逐条 diff：

- **141 个 ref 端点全部在 rust 有完全相同 method+path 的对应**（含参数名一致）。
- **rust 有 ref 无（➕）恰 1 个**：

| ➕ | 端点 | rust 位置 | ref |
|---|---|---|---|
| D-1 | `POST /api/plugin/install` | oc-server/src/handlers/misc.rs:25,377-441；routes.gen.rs（group plugin, 第 4 op） | protocol/src/groups/plugin.ts 仅 list/check/update 三个 |

其余抽查确认的组内对齐：server 3、debug 2、location 2、agent 2、plugin 3(+1)、model 2、provider 2、generate 1、config 3、fs 4、form 1、command/skill/reference 各 1、rpc 1、event 1、credential 5、project 2、mcp 6、integration 11、websearch 2、permission 7、session+message 44、pty 7、persistent-pty 11、shell 5、worktree 4、vcs 6。

## 对比表（逐域逐行）

| 域 | ref（file:line） | rust（file:line） | 判定 |
|---|---|---|---|
| Basic 认证 | auth.ts:36-40（username 固定 "opencode"、首冒号切分、坏 base64→空凭证） | auth.rs:69-92,145-160 | ✓ |
| 会话令牌 | auth.ts:44-61（`expires.HMAC`，key=HMAC-SHA256(pw,"opencode-session-v1")，TTL 30d，timingSafeEqual） | auth.rs:163-226（KDF info、base64url、f64 语义过期比较、常时比较） | ✓ |
| Cookie | auth.ts:63-66（按端口命名）+ authorization.ts:73-79（Origin 同源限制） | auth.rs:230-292（IPv6 bracket 感知） | ✓ |
| auth_token query | authorization.ts:35-39（query 优先） | auth.rs:395-404 | ✓ |
| 401 challenge 条件 | authorization.ts:52-54 | auth.rs:441-449 | ✓ |
| 授权豁免 | authorization.ts:60-63 + process.ts:189-194 | middleware.rs:197-260 | ✓ |
| dispatch 门 | process.ts:172-196 | app.rs:184-226 | ✓（405→404 为有意对齐 Effect 无 405） |
| SSE 帧/队列/溢出 | event-feed.ts:37-39 `data: {json}\n\n`；4096 dropping，溢出仅断该订阅者 | sse.rs:24,31-33,53-110（terminal error 替换 backlog，同语义） | ✓ |
| SSE 心跳/首帧 | 15s `: heartbeat\n\n`，首帧 server.connected，4 响应头 | sse.rs:27,327-363,421-449 | ✓（无 Last-Event-ID——两侧均无重连游标，对齐） |
| 配对 | pairing.ts:16-36（16B base64url、TTL 300s、容量 1000、一次性）+ browser=text/html 分流、302+cookie / JSON token | pairing.rs:19-121 | ✓（容量满逐出策略不可观察差异） |
| CORS | cors.ts:6-31 | middleware.rs:26-80,111-186 | ✓ |
| location 派生 | location.ts:57-65（`location[directory]` query → x-opencode-directory → cwd） | middleware.rs:269-297 | ✓ |
| schema-error 中间件 | reason 截 1024 + InvalidRequestError | request_validation.rs + 挂载顺序 | ✓ |
| session/form-location 中间件 | 含 `sessionID==="global"` MCP elicitation 逃生口 | handlers/common.rs:270-300、form.rs:30-50 | ✓ |
| info/ready 503 | process.ts:198-232 | info.rs:20-49、errors.rs:586-607、readiness.rs | ✓ |
| 端口扫描 | process.ts:110-117（4096 起 EADDRINUSE 递增） | lifecycle.rs:1421-1465 | ✓ |
| PTY REST+票据 | handlers/pty.ts:21-165 | handlers/pty.rs:21-234（x-opencode-ticket:1、origin 先于 consume、60s TTL） | ✓ |
| PTY WS 帧 | REPLAY_CHUNK 64KiB、0x00元帧、1000/4404 | handlers/pty.rs:284-392 | ✓（D-5 分块边界差异） |
| persistent-pty WS | attached/replay/resized{checkpoint}/replay_complete、role/takeover/input_protocol/attachment_id、cursor<0→400 | handlers/persistent_pty.rs:296-600 | ✓（D-6 光标解析差异） |
| MCP 端点 | handlers/mcp.ts:21-118 | handlers/mcp.rs:29-150 | ✓ |
| session 32 ops | handlers/session.ts:37-715 | handlers/session.rs:40-2600 | ✓（stats/move/shell 段按指示未重审） |
| message 游标 | handlers/message.ts:24-68 | handlers/message.rs:35-130 | ✓ |
| credential | selected-last + active 标记、显式 id 冲突 409 | handlers/credential.rs | ✓ |
| integration 9+2 | handlers/integration.ts:23-247 | handlers/integration.rs | ✓ |
| vcs | handlers/vcs.ts:19-102 | handlers/project.rs:1263-1284,1862-1940 | ◐（D-4：provider 硬编码） |
| worktree/project | handlers/worktree.ts、project.ts | handlers/project.rs:36-165,193-860 | ✓ |
| shell | handlers/shell.ts:11-72 | handlers/shell.rs:23-124 | ◐（D-3：DELETE 幂等） |
| fs | handlers/fs.ts:11-55 | handlers/fs.rs:20-176+ | ✓ |
| config/generate/rpc/websearch/permission | 各 handlers | 各 handlers | ✓ |
| /openapi.json | routes.ts:170 | assets.rs:22-26（嵌入 sha 钉死） | ✓ |
| ServerOptions 开关 | options.ts:5-47（database/pty/events.persist/models/config/windows.gitbash/fs.filewatcher/fff/password/app/simulation） | boot.rs/server_process_env.rs | ◐（D-2：simulation/workerd 缺） |
| process.ts 生命周期 | process.ts:64-239 | lifecycle.rs/boot.rs/readiness.rs/safekill.rs（pidfd 安全停机、EADDRINUSE incumbent 协商） | ✓（safekill 为 rust 加强） |

## 行为差异清单 D-n

- **D-1（➕ 端点）** `POST /api/plugin/install` 为 rust 独有：安装+写全局 config+热激活+plugin.updated 事件。ref 无此端点；生成侧同步进入 openapi.json 与 oc-client（methods.gen plugin::install）。
- **D-2（❌ 运行时剖面）** ref `workerd.ts`（Durable Object 剖面：DO sqlite、无执行平面 stub、events.persist 强制 true）与 `fetch.ts`（web-standard fetch handler 嵌入入口）在 rust 无对应；`ServerOptions.simulation` 在 rust 是显式功能性拒绝（server_process_env.rs:28-33），ref 是替换服务图。rust 服务只能以独立进程/axum 监听器形态运行。
- **D-3** `DELETE /api/shell/:id`：ref 吞 Shell.NotFoundError 返回 204；rust 映射 404。删除不存在 shell：ref 204 / rust 404。
- **D-4** `POST /api/vcs/init`：rust provider 白名单硬编码 `git|hg` 并直接 Command 执行；ref 经 `Plugin.awaitActivation` 后由插件注册的 VCS provider 执行，插件自定义 provider（及 unsupported→501 路径）在 rust 不可达。
- **D-5** PTY replay 分块边界：ref 按 JS UTF-16 code unit 切（64KiB 边界可拆代理对）；rust 按 char boundary 切。仅非 BMP 字符边界处可见差异；rust 为修正性偏差。
- **D-6** persistent-pty `cursor` 解析：ref `Number(...)`（接受 "1e2" 等）；rust `usize::parse` 对上述形式回 400。普通十进制一致。
- **D-7（微）** pty.rs:415-424 注释与实现矛盾（实现正确），建议修注释。
- **D-8（微）** persistent-pty 票据 scope 含 directory=进程 cwd（与 /api/pty 的 location-scoped 语义混用同一 Scope 结构）；同进程内自洽。
- **D-9（微）** 配对缓存逐出：ref Effect Cache（TTL+LRU）；rust 逐"最近过期"项。

## 正确性风险 R-n

- **R-1（D-3，中）** shell DELETE 幂等性破坏：先 list 后逐个 delete 的客户端在并发删除/过期清理后会因 404 中止。rust 侧只需在 remove 分支吞 NotFoundError。
- **R-2（D-4，中）** 插件注册的 VCS provider 在 rust 无法经 HTTP 初始化——vcs_init 走 `std::process::Command` 旁路，未来插件 VCS（jj/pijul）将静默 400。
- **R-3（D-1，低）** rust 独有端点进入生成的 openapi.json/客户端：跨实现混用（rust client → ref server）在 install 上得到 404，排障易混淆。建议 openapi 描述标注 `x-rust-extension`。
- **R-4（D-2，低）** workerd/fetch 剖面缺失属已知范围缺口；但 `events.persist` 开关在 rust 未见显式对应——若某部署需关停 durable 事件写入，无服务器级开关。
- **R-5（低）** pairing Set-Cookie 拼接的 expect 依赖跨函数不变量（token 受限字符集），恒成立，无实际风险。

## 覆盖声明

- 已逐行读完：ref `packages/server/src/` 全部 21 文件 + middleware/ 4 + handlers/ 全部 32 文件（session.ts 715 行全文）；protocol/src api.ts 全文、errors.ts 类清单、middleware/ 2 文件、simulation.ts 结构性通读（691 行未逐行——其替换服务图在 rust 无对应）；groups/*.ts 以 141 端点机械提取 + 关键段精读。
- rust 精读：auth.rs、pairing.rs、tickets.rs、middleware.rs、app.rs、readiness.rs、info.rs 全文；sse.rs 核心 400 行；handlers/ 的 mod/config/shell/mcp/message/misc/persistent_pty(段)/pty(段)/project(vcs_init+路由段)/session(段)/form/permission/credential/integration(段)/common(段)；boot/server_process_env/safekill/event_manifest 结构性通读。
- 按指示未深审：handlers/session.rs 的 stats/move/shell 段（C2b 已审）；SSE 事件类型集合（C5a 已审）；routes.gen/validation.gen 仅作端点提取；rust model.rs(1127行)/fs.rs(1517行)/plugins.rs(3171行)/lifecycle.rs(2675行)/errors.rs 未逐行，以路由表+关键错误映射抽查覆盖。
