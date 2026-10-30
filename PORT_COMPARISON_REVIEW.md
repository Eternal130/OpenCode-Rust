# opencode Rust 移植全量对比审查报告

审查基准：`/opt/opencode-ref`（TS 锚定版）vs `/opt/opencode-rust`。六条审查线逐文件逐行覆盖（不抽查、不采信仓库内既有 PORT/REVIEW 文档）：core 会话域、AI/provider 域、server/sdk/plugin 域、CLI/TUI 域、config/auth/工具/MCP 域、基础域+依赖总账。

## 一、功能域对比总表

| 功能域 | ref 实现 | Rust 实现 | 保真度 | 主要差异/缺失 |
|---|---|---|---|---|
| 会话生命周期（inbox/claim/恢复/投影） | Effect fiber + run-coordinator | tokio + 自研 SessionRunCoordinator | ✅ 高 | Rust 加 interrupt_all/list_suspended_before；语义等价 |
| 事件总线/事件溯源 | bus.ts (Effect Stream) | oc-core/bus.rs + SQLite 序列表 | ✅ 高 | 加 deterministic 测试开关 |
| 压缩 compaction（含 native） | State transform 插件体系 | compaction.rs + configure() 直配 | ✅ 高（行为） | ⚠️ state.ts 动态状态框架（运行期热改配置）缺失 |
| 快照/revert/diff/fork/move | git 快照仓 | snapshot.rs (git2) | ✅ 高 | 一致 |
| 指令状态机/instructions | Effect 服务 | context.rs 自由函数 | ✅ 高 | API 形状不同 |
| provider-context legacy media 迁移（<2.0.15） | decode 时升级 | 未见对应分支 | ❌ 疑似缺失 | 旧版原生压缩检查点可能解码失败，需验证 |
| AI 协议层（anthropic/openai-chat/responses/open-responses/gemini/bedrock/vertex/azure/cohere/mistral/xai/copilot/chatgpt） | @ai-sdk + 自研协议 | oc-ai 全自研（reqwest+tungstenite） | ✅ 高（含录制夹具 1:1 对拍） | 修复了 v2 多处误路由 |
| 本地 provider 动态发现（ollama/lmstudio/vllm） | /api/tags、/models 拉取 | ❌ 无对应 | ❌ 缺失 | 本地工作流功能级回归 |
| 长尾 catalog provider（perplexity/gitlab/nvidia/snowflake 等 ~10 个） | aisdk 动态 npm 加载 | 类型化 UnsupportedProtocol 报错 | ⚠️ 折衷 | 干净报错，用户可见 |
| SystemOne 评测协议（experimental） | 有 | ❌ | ❌ 缺失 | 仅影响实验评测面 |
| models.dev 数据 | 运行时 npm 拉取 | 内置快照 + refresh | ✅ 等价 | 离线更友好 |
| HTTP API（141 端点） | effect HttpApi | axum + 生成 routes.gen（sha 锚定 OpenAPI） | ✅ 141/141 全对齐 | ⚠️ 31 个 operationId 前缀漂移（experimental.* 等） |
| 鉴权（Basic/HMAC 票据/端口作用域 cookie/配对/WS query） | node:crypto | hmac/sha2/subtle(ring) | ✅ 逐条等价（含常数时间比较） | — |
| SSE / WS(pty 票据升级) 流式 | Effect Sse/升级 | sse.rs / pty.rs | ✅ 语义一致（挥发、溢出断流、票据顺序） | — |
| 插件系统 | 进程内 import，~20 命名空间 | 进程外 NDJSON JSON-RPC guest（强隔离） | ⚠️ 能力子集 60–70% | ❌ shell/vcs/worktree/websearch/integration/aisdk/tool.execute 未暴露给 guest；tui solid 插件模型不兼容 |
| 配置加载/规范化/热重载/6 源优先级 | config.ts + 22 子 schema | config.rs + normalize.rs | ✅ 高（30 字段 1:1） | BTreeMap 字母序 vs 插入序（仅展示） |
| 权限引擎（最后匹配 wins/saved/deny 短路） | permission.ts | permission.rs | ✅ 高 | — |
| 内置工具（edit/write/read/patch/glob/grep/shell/webfetch/websearch/question/skill/subagent/mcp-resource/codemode…） | tool/plugin/* | tool/*.rs | ✅ 高（逐行为对齐，含 CF 403 回退、三段匹配、模型钩子切换 patch） | registry 校验器为 JSON-Schema 子集 |
| MCP（OAuth/elicitation/协商/超时/重连重放） | 官方 TS SDK | rmcp 3.7 | ✅ 高 | ⚠️ elicitation capability 少 applyDefaults；rmcp 版本漂移需回归 |
| environment/workspace 执行域抽象 | Files 契约 + workspace/driver | ❌ 无 seam，直接 std::fs/tokio::process | ❌ 架构性缺失 | 远程/容器/托管 workspace 不可接入；worktree 策略注册表未移植 |
| LSP | 仅 config schema | 仅 config schema | 平 | 双侧均未实现运行时 |
| CLI 命令面 | 全部命令/flag | clap 全对齐 | ✅ 无缺失 | 差异全为 Rust 超集（completion、--force、2 个升级 method、解析期校验） |
| TUI | opentui/solid | ratatui+crossterm+taffy，双轨（手写+门控 solid） | ✅ 覆盖广 | ⚠️ 默认构建少 21 语法语言（需 all-languages feature）；双轨像素差靠 golden tests；feature-plugins 热路径近似 |
| PTY | 内嵌 @opencode-ai/pty | 独立 oc-pty daemon + vt100 | ✅ 功能对应 | 架构不同（多一进程，需运维文档化） |
| theme | OKLCH/expand/schema/resolve/v1-migrate | oc-theme | ✅ 近无损 | Rust 加内嵌主题/发现/系统模式推断 |
| schema | packages/schema | oc-schema | ✅ 高 | ❌ 显式 TODO：v1 legacy×4、rpc、pty-ticket、event-manifest、legacy-event 等 |
| util | flock/npm/patch/path… | oc-util + 分散自研 | ⚠️ 中 | flock 无 stale-lock 恢复；npm 管理域被 bun 单路径替代（无 Arborist GC/pacote 检查）；artifact mime 表无对应 |
| 语法高亮 | web-tree-sitter 全集 37 | tree-sitter 原生 ×37 | ⚠️ | 21 个 feature 门控，默认包不含 |
| ACP | @agentclientprotocol/sdk 1.6 | vendored crate 3.3 + 20 模块自实现 | ✅ | 版本线不同 |
| workerd 运行时层 / simulation 注入后端 | 有 | ❌ 无 | ❌ | 测试/部署形态缺口，不影响 API 契约 |

## 二、未移植包清单（ref packages 无对应物）

明确不做（ROADMAP）：desktop、gui-extensions、web。
半移植：app（serve/SSE/auth 已做，前端本体范围外）、codemode（已有）、merman（仅诊断级渲染）、latex（无）。
未声明：function、enterprise、identity、posts、storybook、stats、script、session-ui（部分并入 oc-tui）、ui、simulation、http-recorder（Rust 用 cassette/golden 对拍替代）。

## 三、外部依赖对照（双向）

| TS/npm | Rust 替代 | 方式 |
|---|---|---|
| effect（运行时/DI/HttpApi/Stream） | tokio + axum + 自研（coordinator/bus/retry/routes.gen） | 架构性替代 |
| zod / @standard-schema | schemars + 自研子集校验器 | 替代 |
| @ai-sdk/* | oc-ai 全自研 provider 层 | 重写（长尾 provider 不加载） |
| drizzle-orm (#sqlite) | rusqlite (bundled) + 手写 SQL | 替代 |
| @modelcontextprotocol/sdk | rmcp 3.7 | crate 替代 |
| @opentui/{core,solid,keymap}、solid-js | vendored opentui-rust + ratatui/crossterm/taffy | 双轨 |
| @lydell/node-pty、bun-pty | portable-pty + 自研 oc-pty daemon | 重构 |
| tree-sitter wasm ×37 | tree-sitter 原生 crate（4 个 vendored） | 替代 |
| @agentclientprotocol/sdk | vendored agent-client-protocol 3.3 | crate |
| aws4fetch、@smithy/eventstream-codec、@aws-sdk/credential-providers | 手写 sigv4/event_stream/credentials（oc-ai/bedrock） | 自研 |
| google-auth-library | oc-core integration/vertex.rs ADC 链 + ring RS256 | 自研 |
| jose | hmac/sha2/subtle/ring | 替代 |
| @opentelemetry/* | tracing + otel-otlp + tonic | 替代 |
| @parcel/watcher | notify | 替代 |
| jsonc-parser | json5 + 自研 jsonc_edit.rs | 重写 |
| htmlparser2 | 自研 tokenizer（html_markdown.rs） | 重写 |
| diff (jsdiff) | 自研 file_diff.rs | 重写 |
| glob/minimatch | glob + walkdir + 自研 wildcard.rs | 替代 |
| uqr | oc-uqr（自研移植） | 重写 |
| @clack/prompts | dialoguer + clap | 替代 |
| express 侧（@effect/platform-node） | axum + tower-http | 替代 |
| immer/remeda/picocolors/fuzzysort 等 | 自研 / unicode-width / unicode-segmentation | 替代 |
| httpapi-codegen + prettier | tools/openapi-gen + build.rs | 自研生成器 |
| isomorphic-git | git2 + 自研 git 命令层 | 替代 |
| sharp/图像 | image 0.25 | 部分（photon-node 无 1:1） |
| @npmcli/arborist、pacote、@npmcli/config | bun 安装路径（oc-plugin-host bunman） | ❌ 语义漂移：代数 GC、更新检查缺失 |
| proper-lockfile（flock.ts 底层） | 纯 flock(2) | ❌ 无 stale-lock 恢复 |
| @ff-labs/fff-*（feature flags） | — | ❌ 无对应 |
| @opencode/http-recorder | cassette 回放 + golden parity 框架 | 测试基建替代 |
| cross-spawn / which | std::process + 自研 | ⚠️ Windows 等价性未系统验证 |

**Rust 新增（ref 无）**：git2、ring、vt100、dashmap、parking_lot、flate2/brotli-decompressor、jiff、schemars、uuid、rusqlite、taffy、tonic、reqwest(rustls)、tokio-tungstenite、agent-client-protocol(vendored)、opentui-*(git pin)、4 个 vendored tree-sitter 语法（kotlin/toml/vim/vue，crates.io 版本绑定原因）。新增均克制、用途明确。

## 四、风险清单（按优先级）

1. **高（架构）**：environment/workspace 执行域抽象整层缺失——所有文件/进程操作绑死本地；托管/远程执行器路线被阻塞，后续每个工具需二次改造。
2. **高（生态）**：插件宿主 API 面为 TS 子集（60–70%），shell/vcs/worktree/websearch/integration/aisdk/tool.execute 未暴露，存量 TS 插件静默拿到 unknown host method。
3. **高（功能回归）**：本地 provider 动态发现（ollama/lmstudio/vllm）完全缺失。
4. **中**：provider-context legacy media（<2.0.15 checkpoint）迁移疑似缺失——需立即验证 decode_provider_context。
5. **中**：state.ts 动态状态/插件热重配置框架缺失（compaction/snapshot 运行期开关路径）。
6. **中**：31 个 operationId 前缀漂移（operationId 键控的 codegen/网关需映射表）。
7. **中**：TUI 默认构建丢 21 种语法语言；双轨渲染结构性像素差；feature-plugins 近似实现。
8. **中**：oc-schema legacy 系列（v1/rpc/pty-ticket/manifest）显式 TODO 未清。
9. **低-中**：npm 插件管理语义漂移（无 Arborist GC/更新检查）；flock 无 stale-lock 恢复；rmcp 版本漂移需回归；Windows cross-spawn 等价性未验证。
10. **低**：CLI 差异全为超集；鉴权/路由/流式/重试/成本计算逐条等价且有 OpenAPI sha 锚与全量探针测试兜底。

## 五、总评

移植完成度与保真度整体**高**：核心域（会话事件溯源、inbox 生命周期、压缩、快照/revert、权限引擎、内置工具、MCP、HTTP 全路由、AI 协议层、CLI/TUI 主体）均有逐语义对应，且大量以 v2 行号注释锚定 + 录制夹具/golden 对拍/Python 一致性守卫兜底，多处修复了 v2 已知缺陷。剩余风险集中在四类：架构性 seam 缺失（workspace 执行域、插件 API 面、动态状态框架）、旧数据兼容（legacy media、schema v1）、生态长尾（本地 provider 发现、长尾 catalog、feature flags）、以及构建/发布口径（默认语法子集、operationId 前缀）。
