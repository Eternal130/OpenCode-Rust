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

- **运行时选型（Phase 1-2 spike 定夺，硬决策门在 Phase 3 前）**：两个候选 head-to-head——
  - (A) `deno_core`（V8）+ 自建 ESM 加载器 + oxc/swc 转译 + node 内建 shim 层：风险在 node 兼容面（`node:fs`/`child_process` 等 shim 工作量、`aisdk:` 动态 provider 加载 npm 包）。
  - (B) `bun` 子进程宿主：100% Bun 生态兼容（社区插件 npm 依赖直接可用），代价是宿主↔插件 IPC 协议 + 同步 editor 回调/TUI slot 的桥接时延。
  - spike 验收（两候选各跑）：真实社区插件（npm 依赖含 node 内建）；`aisdk:` provider 加载；同步 editor 回调往返；热重载 digest 扫描（oxc 依赖扫描 ≈ `Bun.Transpiler.scan`）；TUI slot 渲染。**未过 spike 前不写任何 core/插件胶水代码。**
- **codemode 归 oc-plugin-host**：v2 中 MCP 工具默认经 codemode（JS `execute` 解释器）分发，与插件运行时是同一个 JS 引擎决策，不是两个问题。
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
- **迁移日志兼容（弃用 sqlx 自带 tracker）**：`sqlx::migrate` 用 `_sqlx_migrations` 表，drizzle 用 `__drizzle_migrations`（自有 hash 格式）。Rust 版实现 drizzle 兼容日志的读/写（同表名、同 hash 语义）。**验收：打开 v2.0.24 产生、含 48 条日志的 DB → 0 条迁移重跑 → 正常续写。**
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
| 2 | LLM SDK+决策门 | oc-ai：协议 **anthropic-messages/openai-chat/openai-responses/openai-compatible 先行，gemini 第五**；流式事件模型 + models.dev 目录 + provider 注册表（bedrock/vertex 列 backlog；github-copilot 与 `aisdk:` 挂钩插件运行时决策）；**Phase 3 前过插件运行时硬决策门** | 录制流回放断言事件序列与 v2 相同；spike 报告+运行时决策记录 |
| 3 | core 地基 A | oc-core：sqlx schema + **drizzle 兼容迁移日志（同表名同 hash，弃 _sqlx_migrations）**、durable 总线（event/event_sequence/projector）、config 发现、**HookRegistry trait 集**、权限规则引擎（wildcard/last-match/ask+Deferred 语义）、file-access | 打开 v2.0.24 DB（48 条日志）→ 0 重跑 → 续写；权限/配置对参照测试全绿 |
| 4 | core 引擎 B | 会话 runner（drain/advanceToStep/runStep/retry/max-steps/compaction）、inbox、14 内置工具（**internal-plugin 形式走 HookRegistry**；tree-sitter shell 扫描、捆绑 rg）、subagent、instructions（AGENTS.md）、revert/snapshot、MCP(rmcp；**codemode 分发语义挂钩插件运行时决策**)、PTY | 对照 harness：同 fixture 会话两版同序事件流与消息树；hook 路径无 JS 运行时也全通 |
| 5 | server | oc-server：openapi-gen 生成 141 路由、SSE /api/event（4096 队列/心跳15s）、Basic+HMAC cookie 认证、pair、PTY WS ×3、ready 门、静态资源（**内嵌参照构建的 app/dist + 逐 HTML CSP hash 复刻**）；oc-client 生成 SDK | /openapi.json 语义 diff=0；**错误路径 4xx golden**；SSE 序列、认证流、service 生命周期 parity |
| 6 | CLI | oc-cli：clap 全命令树（root/**upgrade no-op**/**uninstall no-op**/acp/api/debug×3/auth×6/mcp×4/plugin×5/models/stats/mini/run/session×4/service×6/reload/pair/serve）、service ensure/replace、askpass 隐藏入口、**自动升级默认禁用** | `--help` 树对照；service 管理行为 parity；run/api 端到端 |
| 7 | TUI 基座 | oc-tui：响应 store、taffy 布局层、keymap 引擎（leader/序列/层）、主题接线（**discovery/system 终端检测在此 crate**）、tabs 模型、home/session 路由骨架、SSE 数据层；golden 帧**此前已导出** | 组件级 golden 帧起步（先 5 个核心组件，含 ANSI 样式字节）；键位表与 v2 默认完全一致 |
| 8 | TUI 全量 | composer（textarea+自动补全+历史+stash）、消息流（虚拟滚动/分组/markdown/工具渲染）、20+ 对话框、命令面板、sidebar、diff-viewer、mini 模式、剪贴板/外编($EDITOR)/注意力（bell）；**终端矩阵（xterm-256/screen/tmux）+ resize + 按键回放 harness** | 23 个 v2 快照测试复刻通过 + storybook 场景集 |
| 9 | 插件 | oc-plugin-host（按 Phase 2 决策实现）：运行时+ESM/转译、20 域 Context、hook 顺序/可变对象、transform+reload、KV、RPC、TUI slot 桥（覆盖面=文档化 slot+ui API；renderer-direct 声明为例外）、npm 安装、V1/V2 双格式、codemode、内置插件等同物 | 插件兼容套件（v2 官方测试+社区样本+≥1 真实 TUI 插件）双实现同行为 |
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
