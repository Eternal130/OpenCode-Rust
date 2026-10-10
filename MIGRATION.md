# opencode v2.0.24 → Rust 迁移方案

> 状态：v1.1（已吸收 oracle 门 1 的 12 条整改意见，见 oracle-gate1.md）
> 参照源码：`/opt/opencode-ref`（v2.0.24, commit `e7a34f09bf`，只读 worktree）
> 行为对照基准：用 bun 从参照源码构建的 v2.0.24（**不是**已安装的 v2.0.25 二进制）

## 1. 目标与范围

**目标**：与 opencode v2.0.24 在**功能与外观上一致**，全线 Rust 重写；**插件支持等同**（现有 JS/TS 插件无需修改即可运行）。

**范围内**（v2.0.24 对应包 → 说明）：

| 参照包 | LOC | 移植内容 |
|---|---|---|
| `packages/core` | 65.7k | 会话引擎、工具、权限、事件总线、存储、配置 |
| `packages/ai` | 31.0k | 自研 LLM SDK（协议 + ~60 provider + models.dev 目录） |
| `packages/tui` | 61.7k | 终端 UI（SolidJS+opentui → ratatui） |
| `packages/cli` | 12.2k | `opencode` 命令（含 acp、run、service 等） |
| `packages/server` | 4.0k | HTTP API（141 操作）+ SSE + WS |
| `packages/plugin` | 3.3k | 插件契约（Promise API 为主） |
| `packages/client`+`protocol` | ~21k | OpenAPI 契约 + 生成客户端 |
| `packages/schema` | 5.7k | 共享类型（Effect-Schema → serde+schemars） |
| `packages/theme` | 1.7k | 主题令牌 schema + v1 迁移 + 34 内置主题 |
| `packages/util` | 4.2k | XDG 路径、flock、npm、retry 等工具 |
| `packages/sdk` | 1.0k | 嵌入式宿主（Rust 侧为库形态） |

**范围外**：`desktop`（桌面端）、`gui-extensions`、`app`（Web UI 本体，服务端仅保留静态资源伺服能力）、`function`（Cloudflare Worker，与产品无关）、`plugin-browser`（依赖桌面 CDP，列为可选后续）、Web UI 的移植。

## 2. 总体架构决策

### 2.1 工作区与 crate 布局

单一 workspace，产出单一 `opencode` 二进制（与 v2 相同形态：一个命令既是 CLI 又能拉起 TUI/服务）。

```
opencode-rust/
├── crates/
│   ├── oc-util/        # global-roots(XDG)、flock、retry、hash、glob、npm 壳、wildcard
│   ├── oc-schema/      # packages/schema 移植：serde + schemars，tagged union 事件/消息类型
│   ├── oc-theme/       # packages/theme 移植：令牌 schema、v1 迁移(oklch)、34 内置主题、resolve 管线
│   │                   #   （主题 discovery 与 system 主题需终端 I/O，归 oc-tui）
│   ├── oc-ai/          # packages/ai 移植：协议层(anthropic/openai/gemini/bedrock…)、流式事件、
│   │                   #   provider 注册表、models.dev 目录
│   ├── oc-core/        # packages/core 移植：DB(sqlx/SQLite)、总线、会话 runner、14 内置工具、
│   │                   #   权限引擎、file-access、subagent、instructions、MCP(rmcp)、PTY
│   ├── oc-plugin-host/ # 插件宿主：JS 运行时（deno_core vs bun 子进程，Phase 1-2 spike 定夺）+ 20 域
│   │                   #   Context 桥接 + 热重载 + codemode JS 解释器（MCP 工具默认走 codemode）
│   │                   #   依赖方向：plugin-host → core（实现 core 的 HookRegistry trait，组合根注入）
│   ├── oc-server/      # packages/server 移植：axum 路由(由 openapi.json 驱动)、SSE 事件流、
│   │                   #   Basic+HMAC 认证、PTY WebSocket、静态资源
│   ├── oc-client/      # 生成的 Rust HTTP/SSE 客户端 SDK（由 openapi.json 代码生成）
│   ├── oc-tui/         # packages/tui 移植：ratatui+crossterm、taffy flex 布局、keymap 引擎、
│   │                   #   组件（tabs/session/composer/dialogs/palette/mini 模式）
│   └── oc-cli/         # packages/cli 移植：clap 命令树、service 管理、TUI 引导
├── tools/
│   ├── openapi-gen/    # openapi.json → Rust 路由/客户端/类型 生成器
│   └── parity/         # 一致性验收 harness（对照 v2.0.24 参照构建）
└── MIGRATION.md
```

依赖方向与 v2 相同：`schema ← core ← server`；`client` 依赖 `schema`（不依赖 core/server）；`cli` 组合一切。

### 2.2 关键技术映射

| v2 (TS/Bun) | Rust | 说明 |
|---|---|---|
| Effect services / Layer | trait + `Arc<T>` 注册表（按 location/global 作用域） | v2 的 `makeLocationNode` 惯用法映射为 per-location service 工厂 |
| Effect Fiber/Deferred/Semaphore/Queue | tokio `JoinSet`/`oneshot`/`Semaphore`/`mpsc` | |
| Effect Stream | `futures::Stream` / `tokio_stream` | |
| Effect-Schema | serde + schemars（JSON Schema 输出对齐） | 工具 I/O 在 JS 边界接受 standard-schema 等价物 |
| Bun SQLite + drizzle | sqlx (SQLite) + 手工移植 48 个 migration | **保持 DB schema 逐列一致**，直接可开 v2 数据目录 |
| node:http (Effect HttpApi) | axum + hyper | openapi.json 为契约源 |
| opentui（Zig 渲染/flexbox/RGBA） | ratatui + crossterm + **taffy**（flex 布局对齐） | 自建 Box/Text 组件树映射 opentui 语义 |
| SolidJS 响应式 | 自建轻量响应应 store（signal/memo + 订阅重绘） | 不引 UI 框架，事件驱动重绘 |
| @opentui/keymap（leader/序列/层） | 自建 keymap 引擎（同语义：层、模式、leader 超时 2000ms） | |
| tree-sitter（bash/powershell 扫描） | tree-sitter + tree-sitter-bash/powershell crate | 权限资源保守映射逻辑 1:1 |
| @modelcontextprotocol/sdk | rmcp（官方 Rust SDK） | |
| node-pty / persistent-pty | portable-pty + tokio | |
| @parcel/watcher | notify | |
| isomorphic git（git.ts 758 行） | git2（libgit2） | vcs 插件化接口保留 |
| diff | similar | |
| oklch / 主题 | palette（oklab/oklch） | v1→v2 迁移色域行为对齐 |
| string-width/east-asian-width | unicode-width + 对照测试 | CJK 宽度对齐是外观一致性关键 |
| fuzzysort | nucleo（或 fuzzy-matcher，评分排序需对照校准） | |
| uqr (QR) | qrcode | |
| Effect CLI | clap | 命令树逐条映射（见 §5） |
| immer | im / 手写 | |
| photon (图片缩放) | image | |

### 2.3 插件系统方案（等同性关键）

v2 插件是**进程内 JS/TS ESM 模块**（Bun 动态 import + npm 解析 + watch 热重载），Context 约 20 域。Rust 等同支持：

- **运行时选型（已 spike，预选 (B) 待门 2 确认）**：head-to-head 结论 ——
  - (A) `deno_core`：同步回调 0.8µs 极快，但 **结构性失败**：CJS 依赖（lodash 等）ESM link 即死、node 内建兼容面是多年工程；V8 体积 54MB/260 crates。
  - (B) `bun` 子进程宿主（**推荐**）：社区插件/CJS/node 内建原生全通（v2 语义同源）；同步回调经阻塞 RPC 实测 49-54µs avg / 0.5ms p99（预算内）；宿主协议注意：写锁与阻塞回复的死锁规则成文。分发注意：运行期依赖系统 bun（缺失时插件功能降级并明示，不阻塞核心功能）。
  - 证据：spike-plugin-runtime.md（A 2.5/5 vs B 5/5 通过 oracle 五项标准）。
- **Phase 9 绑定约束（oracle 门 2 确认 (B) 时下达）**：① bun 分发是产品决策，Phase 9 入口前定（托管钉版下载+校验 vs 内嵌 vs 仅降级模式）；无 bun 时插件/codemode/`aisdk:` 优雅降级且为 CI 测试路径。② 死锁协议规范成文（IO 线程不持写锁阻塞、guest→host 同步调用在 IO 线程应答、request-id 状态机）+ 嵌套/重入 fuzz 测试。③ copy-in/copy-out vs v2 引用可变——写入插件 parity 例外清单。④ 插件状态留在 guest 侧（脏增量/reload 时变换/渲染路径不 editor-RPC）为 API 设计规则。⑤ 钉死 bun 版本（升级会改变插件可见的 node 兼容行为）。
- **codemode 归 oc-plugin-host**：v2 中 MCP 工具默认经 codemode（JS `execute` 解释器）分发，与插件运行时是同一个 JS 引擎决策——选 (B) 后 codemode 跑在 bun 宿主内，降级阶梯第 2 级解除。
- **宿主桥（Rust 侧语义）**：有序 hook 注册表（注册序顺序执行、可变事件对象）、transform 域（同步 editor 回调 + reload 失效）、作用域化 Registration/dispose、plugin-scoped KV storage、事件流 AsyncIterable、RPC 端口（`/api/rpc/{id}/{method}` + `rpc.*` 事件）。
- **core 缝隙（Phase 3 即建）**：oc-core 定义 `HookRegistry`/扩展点 trait 集（session.prompt / tool.execute.before|after / permission.evaluate / shell.create.before / http.request|response 等）；内置工具按 v2 方式写成 internal plugin 走同一注册表；oc-plugin-host 实现 trait 并在组合根（server/cli/sdk）注入。Phase 4 验收要求：即使无 JS 运行时，hook 路径也经注册表全通。
- **API 面**：以 Promise API（`{id, setup(ctx)}`）为插件作者面；V1+V2 双格式按 v2.0.24 文档兼容。
- **TUI 插件桥（覆盖面成文）**：已文档化 slot 路径（`app`/`home.footer`/`session.composer.top`/`session.panel`/`sidebar.*`）+ `ui.dialog/toast/router/panel/tabs` + `keymap.layer` + `markdown.registerCodeBlockRenderer` + storage store。**renderer-direct 插件（直接拿 CliRenderer）显式声明为 parity 例外**；兼容套件含 ≥1 个真实 TUI 插件。

### 2.4 版本与互操作

- 二进制自报版本 `2.0.24-rust.N`？**否** —— 自报 `2.0.24`，保证与 v2.0.24 的 TUI/CLI/服务互替换（service 版本协商是精确字符串匹配）。`opencode --version` 输出 `opencode v2.0.24`，另以 `--version-rust` 暴露移植构建号。
- **Rust 版禁用自动升级**（v2 守护进程 10 分钟一轮后台更新会拉最新 TS 版自我覆盖，版本伪装下等于静默自毁）：默认 `OPENCODE_DISABLE_AUTOUPDATE` 语义置真；`opencode upgrade`/`uninstall` 输出明确提示的 no-op。绝不采用 >2.0.24 的 TS 发布物。
- 注册文件 `service.json`、端口规则（49374/0xc0df/通道哈希）、`/api/info` 探活、PTY handoff 均按 v2.0.24 行为实现；**Rust 版与 TS 版服务可互相顶替**（含版本一致时的 incumbent 幂等启动）。

## 3. 数据与存储兼容

- SQLite：逐列复刻 v2.0.24 drizzle snapshot（`packages/core/schema.json`，209 DDL 实体；表：project/workspace/worktree/session_v2/session_message/session_pending/session_inbox/instruction_*/event/event_sequence/permission/credential/account*/kv）。
- **迁移日志（侦察修正，v2.0.24 实况）**：自定义 `migration(id TEXT PK, time_completed INTEGER)` 表，**无 hash 列**，id 存在即视为已应用；id 格式 `YYYYMMDDHHMMSS_slug`，48 条严格数组序。新库引导 = 单事务执行 schema.gen 全量 DDL + 一次性写入 48 个 id（不逐条跑）；旧库 = 从 `__drizzle_migrations` 继承（name 列变体直插；hash/created_at 变体按时间戳前缀匹配，不匹配即 die）。并发 = 按文件路径的 Semaphore(1)。**验收：打开 v2.0.24 产生、含 48 条日志的 DB → 0 条重跑 → 续写；空库引导后与 v2 新建库逐表一致。**
- 文件布局：`~/.local/share/opencode`（db/log/repos/snapshot）、`~/.local/state/opencode/service.json`、`~/.config/opencode`（config/plugins/themes/AGENTS.md）全部一致。
- 事件日志：持久事件按 aggregate `seq` 写 `event`/`event_sequence`，projector 重建语义一致（崩溃恢复、stale tool call/compaction 结算）。

## 4. 验证策略（一致性如何被证明）

1. **单元**：每 crate cargo-nextest；纯逻辑测试从 v2 测试目录移植（`test/util/`、`test/mini/*.shared` 等无渲染依赖用例直接翻译）。
2. **确定性注入（parity 模式）**：两实现都支持注入 clock/RNG/uuid 种子（消抖动重试 jitter、事件 id、时间戳、后台标题生成时序）；配**归一化规范**：明确哪些字段重写（id/created/耗时）、哪些必须逐字匹配（类型名/顺序/内容）。
3. **契约**：`tools/openapi-gen` 生成的路由挂载后，`GET /openapi.json` 与参照 `packages/protocol/openapi.json` 做**语义 diff**（路径/方法/schema 数量与形状；141 操作/256 schema）。**另加错误路径 golden**：schema-error 中间件的 4xx 响应体逐字对照（openapi-diff 抓不到）。
4. **行为对照（golden）**：`tools/parity` 以 bun 从源码构建的 v2.0.24（`/tmp/opencode/ref`，`OPENCODE_VERSION=2.0.24`）为参照；对同一输入分别驱动两版，比对：HTTP 响应 JSON、SSE 事件序列（归一化后）、DB 终态、CLI stdout。
5. **视觉**：fixture → 确定性渲染 → golden 帧。比对**不止文本网格**：ANSI 样式字节级 diff（颜色/属性），truecolor→256→16 随 TERM 降级矩阵，终端矩阵（xterm-256color/screen/tmux）+ resize；宽窄两档宽度；**grapheme/emoji/ZWJ/CJK 宽度**专测（`string-width` 与 `unicode-width` 在边界不一致，需对齐 v2 行为）。golden 帧在 Phase 7 前从参照 Bun 快照导出，不是 Phase 7 中临时做。
6. **交互回放**：按键事件回放 harness（leader 2000ms 超时、命令面板、对话框时序）驱动两版 TUI 状态机比对。
7. **插件**：以 v2.0.24 `packages/plugin/test` 与社区典型插件（tool 注册、session hook、permission hook、主题、**TUI slot ≥1 个真实插件**）组成兼容套件，在两个实现上跑同套脚本断言相同行为。
8. **门禁**：每阶段收尾跑 `cargo fmt --check && cargo clippy -D warnings && cargo nextest` + 该阶段 parity 子集；证据记入 deepwork progress。

## 5. 阶段划分（每阶段 = 一个交付边界 + oracle 评审门）

| # | 阶段 | 内容 | 验收要点 |
|---|---|---|---|
| 0 | 侦察+方案 | 四张架构图 + 本文档 | oracle 门 1（已过，整改吸收为 v1.1） |
| 1 | 地基+spike | workspace、oc-util（含确定性注入 trait：Clock/Rng/IdGen）、oc-schema（消息/事件/agent/config 全类型）、oc-theme（v2 schema+34 主题+v1 迁移+resolve）、insta 快照基建、tools/parity 骨架+归一化规范+参照构建钉版（`OPENCODE_VERSION=2.0.24`）；**插件运行时 spike 启动：deno_core+shims vs bun 子进程 head-to-head** | schema 类型与参照 JSON Schema 对齐；主题解析输出与 v2 相同输入逐字节一致；parity 骨架跑通一个端到端样例 |
| 2 | LLM SDK+决策门 | oc-ai：契约锚定 `packages/ai/src/schema/{events,options}.ts`（**Usage 方向翻转不变量：Anthropic/Bedrock 求和、OpenAI/Gemini 做差、clamp；providerMetadata 逐字节往返**）；Route 组合层 + **Transport 缝 = `Arc<dyn Transport>`（进程无关，为 (B) 子进程 `aisdk:` 留位）**；协议 anthropic-messages/openai-chat/openai-responses/openai-compatible 先行，gemini 第五；**274 个 cassette 逐字复用（wiremock 走真实 route 栈）+ 解析器边界语料**；错误分类学+重试判定+partial-json 测试随协议移植；bedrock/vertex backlog；github-copilot 与 `aisdk:` 挂钩插件运行时（已定 (B)） | 录制流回放断言事件序列与 v2 相同；spike 报告+运行时决策记录（已确认 (B)） |
| 3 | core 地基 A | oc-core：sqlite 单连接封装（pragma 全套 WAL/NORMAL/busy5000/cache-64M/FK/0600）+ **自定义 migration(id,time_completed) 日志（含 v2 重开续用验收：Rust 写入 → bun v2 重开 → 续用）+ 空库全量 DDL 引导 + 遗产继承**、durable 总线（**persist-then-publish 不变量成文 + 混沌重放测试**；聚合 KeyedMutex、事务内 projector、log/replay/follow）、config 发现（六级合并/{env}{file}/jsonc/normalize+诊断/watch 100ms 防抖）、**HookRegistry trait 集（async-first；同步 editor 桥由 plugin-host 内部实现）**、权限规则引擎（wildcard/last-match/deny 短路/ask+oneshot 停泊/批拒批準/Deferred 语义）、file-access、**location 服务图（DashMap+Arc、失效/关闭规则成文、TTL 扫描器）**、**provider 目录装配（models.dev 快照钉版嵌入+派生、variant 叠加、facade configure→oc-ai Route 绑定、未移植 provider 选中→干净类型化错误）**；parity：ai-dump 扩展捕获 TS 请求体 + 每协议族 body golden（缓存前缀等价性） | 打开 v2.0.24 DB（48 条日志）→ 0 重跑 → 续写 + Rust 写→v2 重开续用；权限/配置对参照测试全绿；bus 不变量混沌测试绿；未移植 provider 类型化报错 |
| 4 | core 引擎 B | **按门 4 建议分 5 波**——Wave 0: graph v2（持具体 Arc\<Bus\>/Arc\<Database\>、真实 per-location 服务、可失败构建）+ InternalPlugin pre/post 槽位 + EventSink 异步化（replied 先于 Deferred 解析）+ SqliteSavedGrantStore/sqlite ProjectStore + 事件定义集中化 + runner 并发设计笔记（publisher 不变量/取消语义/awaitSettlement）；Wave A: projector 全 34 处理器 + store + inbox + execution claim/release（bun 参照 DB 可证）；Wave B: runner 全套（publisher/step/retry/compaction/drain）+ `prompt_fake_llm` 场景回放；Wave C: 14 内置工具（internal-plugin 走 HookRegistry，输出格式逐字符串对齐）+ instructions + revert/snapshot（git 对象库）+ tree-sitter shell 扫描 + 捆绑 rg；Wave D: MCP(rmcp) + subagent + PTY 守护进程 | Wave A 对照参照 DB 投影全等；Wave B fake_llm 事件序列 golden 回放；hook 路径无 JS 运行时全通；工具行为单测对齐 |
| 5 | server | **Wave 0（门 5 整改，oc-core 内）**：组合根 Engine/App（boot: config→db→bus→map；per-location：ToolServices+内置插件(v2 pre 序)+MCP 注册表+Forms+Shell/Jobs+skill 表+AGENTS.md 注入+with_snapshot+真 drain）+ 中断守卫区（watch 进 attempt()、偏置锁存、终末化完整执行、铃响→join→结算→发布）+ truncate/error_type/title-small/anthropic-compact 接线 + SSE feed 设计笔记 + 中断 parity golden；Wave 1: openapi-gen 141 路由（钉版 openapi.json 生成，语义 diff=0）、SSE /api/event（4096/15s/背压策略）、Basic+HMAC/pair、PTY WS ×3（对进程内 PTY；重启交接语义差异成文）、ready 门、静态资源（内嵌 app/dist+CSP hash）；oc-client 生成 SDK；`command` 路由显式 Phase 9 延后 | Wave 0 全栈 e2e 经组合根 + 中断 golden；/openapi.json 语义 diff=0；错误路径 4xx golden；SSE 序列、认证流、service 生命周期 parity → **M1：官方 v2.0.24 TUI 可用** |
| 6 | CLI | oc-cli：clap 全命令树（root/**upgrade no-op**/**uninstall no-op**/acp/api/debug×3/auth×6/mcp×4/plugin×5/models/stats/mini/run/session×4/service×6/reload/pair/serve）、service ensure/replace、askpass 隐藏入口、**自动升级默认禁用** | `--help` 树对照；service 管理行为 parity；run/api 端到端 |
| 7 | TUI 基座 | oc-tui：响应 store、taffy 布局层、keymap 引擎（leader/序列/层）、主题接线（**discovery/system 终端检测在此 crate**）、tabs 模型、home/session 路由骨架、SSE 数据层；golden 帧**此前已导出** | 组件级 golden 帧起步（先 5 个核心组件，含 ANSI 样式字节）；键位表与 v2 默认完全一致 |
| 8 | TUI 全量 | composer（textarea+自动补全+历史+stash）、消息流（虚拟滚动/分组/markdown/工具渲染）、20+ 对话框、命令面板、sidebar、diff-viewer、mini 模式、剪贴板/外编($EDITOR)/注意力（bell）；**终端矩阵（xterm-256/screen/tmux）+ resize + 按键回放 harness** | 23 个 v2 快照测试复刻通过 + storybook 场景集 |
| 9 | 插件 | **按门 11 波次**——波 1 宿主脊柱（bun 子进程 spawn/监管、IPC 帧、死锁规则、注册期同步桥、post-ingest 事件抽头、硬超时；验收=hello-world 注册命令+收事件）；波 2 旗舰 bring-up（oh-my-opencode-slim 于最小 Context，按需实现域；**bun 分发决策先于此波**；冻结安装）；波 3 TUI slot 桥（DialogKind::Plugin/GroupKind 规则表/SidebarSections/Toast/footer/home；文本级 transform 声明边界）；波 4 泛化+加固（codemode/慢插件 fuzz/事件序测试）；G11-F1 于广播契约内解决 | hello-world 插件；**旗舰 oh-my-opencode-slim v3.0.3 全注册面+跑一个 skill 端到端**；v2 官方插件测试移植；≥1 真实 TUI slot 插件 |
| 10 | 加固 | openapi 等价门、全量 parity 回归、DB 互通、跨版本 service 顶替（rust↔TS 互替换）、打包（release 二进制+安装脚本）、文档 | 全部门禁绿；oracle 终审 |

**执行纪律**：每阶段内部再分并行 fixer 泳道（按 crate/目录划分写所有权）；TUI 视觉工作路由 @designer/具备设计判断的泳道；阶段收尾 = 验证证据 + 进度文件 + oracle 门 + git 提交。

## 6. 主要风险与对策

| 风险 | 对策 |
|---|---|
| Effect 语义映射错位（并发顺序、中断、作用域清理） | 以行为对照 harness 兜底：同输入比事件序列；先写语义笔记（scope/fiber↔tokio 映射表）再动 core |
| opentui 外观像素级不一致（flex 布局/RGBA/半块字符） | taffy 复刻 flexbox；golden 帧从参照导出而非手写；宽度/CJK/grapheme 专门对照测试 |
| 插件运行时（node 兼容面/npm 生态/同步桥） | Phase 1-2 一周级 head-to-head spike（deno_core+shims vs bun 子进程），Phase 3 前硬决策门；未过 spike 不写胶水 |
| Phase 4/8 体量被低估（runner+14 工具+MCP ≈35k；composer+20 对话框+mini ≈25k） | 见下方降级阶梯，触发即执行并记录 |
| ~60 provider 面广 | 多数收敛到 openai-compatible 配置；协议层只维护 4-6 个；bedrock/vertex 列 backlog |
| v2.0.25 与 .24 行为差 | 一切对照以源码构建的 v2.0.24 为准，绝不引用线上 2.0.25 的行为当规范 |
| 版本伪装 + 自动升级自毁 | Rust 版默认禁自动升级；upgrade/uninstall 为带提示的 no-op |
| 规模（~190k LOC TS） | 严格阶段门控 + parity 工具先行（Phase 1 就建 harness，越早能对照越省返工） |

**降级阶梯**（触发条件：对应阶段预估超期 >50% 或 spike 失败；逐级下调并在进度文件记录差异，mini 与 acp 顺序不可互换除非用户确认）：
1. `plugin-browser`（已出范围）
2. codemode → 若插件运行时选 bun 子进程则降级为「MCP 工具直执行，记录行为差异」
3. `acp` 命令
4. github-copilot 自定义模型 + `aisdk:` 动态 provider
5. mini TUI 模式（保留非交互 `opencode run`）

**Web UI 静态资源**：内嵌对象 = 参照源码（/tmp/opencode/ref）构建出的 `packages/app/dist` 产物；伺服路径与逐 HTML CSP hash 复刻 `cli/src/app-assets.ts` 行为；Web UI 本体不移植。

## 7. 里程碑定义（完成标准）

- M1（Phase 5 末）：Rust server 可被 v2.0.24 的官方 TUI 当作它的服务端使用（含认证/SSE/会话）。
- M2（Phase 8 末）：Rust TUI 可作为 v2.0.24 服务的客户端完成全部日常工作流；`opencode run` 等价。
- M3（Phase 9 末）：社区 JS 插件不修改直接可用。
- M4（Phase 10 末）：全量 parity 套件绿，release 构建。
