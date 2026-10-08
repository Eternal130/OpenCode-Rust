# opencode v2.0.24 → Rust 迁移方案

> 状态：v1（待 oracle 评审门 1）
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
│   ├── oc-ai/          # packages/ai 移植：协议层(anthropic/openai/gemini/bedrock…)、流式事件、
│   │                   #   provider 注册表、models.dev 目录
│   ├── oc-core/        # packages/core 移植：DB(sqlx/SQLite)、总线、会话 runner、14 内置工具、
│   │                   #   权限引擎、file-access、subagent、instructions、MCP(rmcp)、PTY
│   ├── oc-plugin-host/ # 插件宿主：deno_core 内嵌 JS 运行时 + 20 域 Context 桥接 + 热重载
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

- **运行时**：内嵌 `deno_core`（V8）。TS/JS 荷载在加载时经 `swc`/`oxc` 转译；实现 ESM 加载器（node 风格解析：相对、裸名→node_modules、file://）。
- **npm 安装**：`opencode plugin add` / config 声明的 npm/git 包，通过调用系统 `npm`（或 bun）安装到 opencode 数据目录，语义对齐 v2 的 arborist 路径（精确版本/git hash 固定）。
- **宿主桥**：Rust 侧重写宿主语义 —— 有序 hook 注册表（注册序顺序执行、可变事件对象）、transform 域（同步 editor 回调 + reload 失效）、作用域化 Registration/dispose、plugin-scoped KV storage、事件流 AsyncIterable、RPC 端口（`/api/rpc/{id}/{method}` + `rpc.*` 事件）。
- **API 面**：以 Promise API（`{id, setup(ctx)}`）为插件作者面；Effect 形态在 JS 侧兼容（宿主不依赖 Effect）。V1+V2 双格式按 v2.0.24 文档兼容。
- **TUI 插件**：slot 树 claims（`app`/`home.footer`/`session.panel`/`sidebar.*` 等已文档化路径）+ keymap/theme/storage 注入；JS 侧渲染经受限组件桥（box/text 原语）映射到 oc-tui。
- **备选方案**（oracle 评审点）：deno_core + 自建加载器（推荐）vs quickjs（更轻但 npm/TS 生态弱）vs 子进程 bun（破坏 in-process 语义）。风险：deno_core 拉入 V8 体积大、构建重；如果不可接受，回退 quickjs + esbuild 预转译。

### 2.4 版本与互操作

- 二进制自报版本 `2.0.24-rust.N`？**否** —— 自报 `2.0.24`，保证与 v2.0.24 的 TUI/CLI/服务互替换（service 版本协商是精确字符串匹配）。`opencode --version` 输出 `opencode v2.0.24`，另以 `--version-rust` 暴露移植构建号。
- 注册文件 `service.json`、端口规则（49374/0xc0df/通道哈希）、`/api/info` 探活、PTY handoff 均按 v2.0.24 行为实现；**Rust 版与 TS 版服务可互相顶替**（含版本一致时的 incumbent 幂等启动）。

## 3. 数据与存储兼容

- SQLite：逐列复刻 v2.0.24 drizzle snapshot（`packages/core/schema.json`，209 DDL 实体；表：project/workspace/worktree/session_v2/session_message/session_pending/session_inbox/instruction_*/event/event_sequence/permission/credential/account*/kv）。
- Migration 体系：从 48 个 drizzle migration 推导等价 sqlx migration 集；**验收**：Rust 版能打开并继续写一个 v2.0.24 产生的数据目录（含 WAL）。
- 文件布局：`~/.local/share/opencode`（db/log/repos/snapshot）、`~/.local/state/opencode/service.json`、`~/.config/opencode`（config/plugins/themes/AGENTS.md）全部一致。
- 事件日志：持久事件按 aggregate `seq` 写 `event`/`event_sequence`，projector 重建语义一致（崩溃恢复、stale tool call/compaction 结算）。

## 4. 验证策略（一致性如何被证明）

1. **单元**：每 crate cargo-nextest；纯逻辑测试从 v2 测试目录移植（`test/util/`、`test/mini/*.shared` 等无渲染依赖用例直接翻译）。
2. **契约**：`tools/openapi-gen` 生成的路由挂载后，`GET /openapi.json` 与参照 `packages/protocol/openapi.json` 做**语义 diff**（路径/方法/schema 数量与形状；141 操作/256 schema）。
3. **行为对照（golden）**：`tools/parity` 用 bun 构建 `/opt/opencode-ref` 得到精确 v2.0.24 参照二进制；对同一输入（fixture 会话、配置、插件样本）分别驱动参照版与 Rust 版，比对：HTTP 响应 JSON、SSE 事件序列（归一化 id/时间戳后）、DB 终态、CLI stdout。
4. **视觉**：复刻 v2 的 Bun 快照测试体系 —— fixture 数据 → 确定性渲染 → golden 帧（含换行/宽度/CJK）。首版 golden 由参照版在其 headless 渲染测试中导出（`test/cli/tui/__snapshots__` 23 个组件测试为起点），Rust 渲染管线输出同格式比对；宽窄两种终端宽度。
5. **插件**：以 v2.0.24 `packages/plugin/test` 与社区典型插件（tool 注册、session hook、permission hook、主题、TUI slot）组成兼容套件，在两个实现上跑同套脚本断言相同行为。
6. **门禁**：每阶段收尾跑 `cargo fmt --check && cargo clippy -D warnings && cargo nextest` + 该阶段 parity 子集；证据记入 deepwork progress。

## 5. 阶段划分（每阶段 = 一个交付边界 + oracle 评审门）

| # | 阶段 | 内容 | 验收要点 |
|---|---|---|---|
| 0 | 侦察+方案 | 四张架构图 + 本文档 | oracle 门 1（本档） |
| 1 | 地基 | workspace、oc-util、oc-schema（消息/事件/agent/config 全类型）、oc-theme（v2 schema+34 主题+v1 迁移+resolve）、insta 快照基建 | schema 类型与参照 JSON Schema 对齐；主题解析输出与 v2 相同输入逐字节一致 |
| 2 | LLM SDK | oc-ai：协议 anthropic-messages/openai-chat/openai-responses/openai-compatible/gemini/bedrock-converse + 流式事件模型 + models.dev 目录 + provider 注册表（~60 provider 元数据，先通 4 个主流协议通道，其余 provider 配置化收敛） | 用录制流（vcr 式 HTTP fixture）回放断言事件序列与 v2 相同 |
| 3 | core 地基 A | oc-core：sqlx schema+migrations、durable 总线（event/event_sequence/projector）、config 发现（opencode.json/jsonc、向上walk、.opencode/.claude/.agents）、权限规则引擎（wildcard/last-match/ask+Deferred 语义）、file-access | DB 与 v2 互通验收；权限/配置对参照测试用例全绿 |
| 4 | core 引擎 B | 会话 runner（drain/advanceToStep/runStep/retry/max-steps/compaction）、inbox、14 内置工具（含 tree-sitter shell 扫描、捆绑 rg）、subagent、instructions（AGENTS.md）、revert/snapshot、MCP(rmcp)、PTY | 对照 harness：同 fixture 会话在两版产生同序事件流与消息树；工具行为单测对齐 |
| 5 | server | oc-server：openapi-gen 生成 141 路由、SSE /api/event（4096 队列/心跳15s）、Basic+HMAC cookie 认证、pair、PTY WS ×3、ready 门、静态资源；oc-client 生成 SDK | /openapi.json 语义 diff=0；parity：SSE 事件序列、认证流、service 生命周期（注册/顶替/stdio 租约） |
| 6 | CLI | oc-cli：clap 全命令树（root/upgrade/uninstall/acp/api/debug×3/auth×6/mcp×4/plugin×5/models/stats/mini/run/session×4/service×6/reload/pair/serve）、service ensure/replace 逻辑、askpass 隐藏入口 | `--help` 树对照；service 管理行为 parity；run/api 端到端 |
| 7 | TUI 基座 | oc-tui：响应 store、taffy 布局层、keymap 引擎（leader/序列/层）、主题接线（含 system 主题、终端暗亮检测）、tabs 模型、home/session 路由骨架、SSE 数据层 | 组件级 golden 帧起步（先 5 个核心组件）；键位表与 v2 默认完全一致 |
| 8 | TUI 全量 | composer（textarea+自动补全+历史+stash）、消息流（虚拟滚动/分组/markdown/工具渲染）、20+ 对话框、命令面板、sidebar、diff-viewer、mini 模式（`opencode run`/mini 的 18k LOC 前端）、剪贴板/外编($EDITOR)/注意力（bell） | 23 个 v2 快照测试全部复刻通过 + storybook 场景集 |
| 9 | 插件 | oc-plugin-host：deno_core+转译+ESM 解析、20 域 Context、hook 顺序/可变对象、transform+reload、KV、RPC、TUI slot 桥、npm 安装、V1/V2 双格式、内置插件（internal.ts 等同物） | 插件兼容套件（v2 官方测试+社区样本）双实现同行为 |
| 10 | 加固 | openapi 等价门、全量 parity 回归、DB 互通、跨版本 service 顶替（rust↔TS 互替换）、打包（release 二进制+安装脚本）、文档 | 全部门禁绿；oracle 终审 |

**执行纪律**：每阶段内部再分并行 fixer 泳道（按 crate/目录划分写所有权）；TUI 视觉工作路由 @designer/具备设计判断的泳道；阶段收尾 = 验证证据 + 进度文件 + oracle 门 + git 提交。

## 6. 主要风险与对策

| 风险 | 对策 |
|---|---|
| Effect 语义映射错位（并发顺序、中断、作用域清理） | 以行为对照 harness 兜底：同输入比事件序列；先写语义笔记（scope/fiber↔tokio 映射表）再动 core |
| opentui 外观像素级不一致（flex 布局/RGBA/半块字符） | taffy 复刻 flexbox；golden 帧从参照导出而非手写；宽度/CJK 专门对照测试 |
| deno_core 集成重（V8 构建/体积） | Phase 9 前做 1 天 spike 验证加载器+转译+桥；回退预案 quickjs |
| ~60 provider 面广 | 多数收敛到 openai-compatible 配置；逐 provider 用 models.dev 元数据驱动，协议层只维护 4-6 个 |
| mini 模式 18k LOC 第二前端 | 放 Phase 8 后半，先保证主 TUI + `opencode run` 基本面 |
| v2.0.25 与 .24 行为差 | 一切对照以源码构建的 v2.0.24 为准，绝不引用线上 2.0.25 的行为当规范 |
| 规模（~190k LOC TS） | 严格阶段门控 + parity 工具先行（Phase 1 就建 harness，越早能对照越省返工） |

## 7. 里程碑定义（完成标准）

- M1（Phase 5 末）：Rust server 可被 v2.0.24 的官方 TUI 当作它的服务端使用（含认证/SSE/会话）。
- M2（Phase 8 末）：Rust TUI 可作为 v2.0.24 服务的客户端完成全部日常工作流；`opencode run` 等价。
- M3（Phase 9 末）：社区 JS 插件不修改直接可用。
- M4（Phase 10 末）：全量 parity 套件绿，release 构建。
