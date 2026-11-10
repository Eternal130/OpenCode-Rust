# opencode v2.0.24 → Rust 移植逐项功能对比总表

> 锚定参考：/opt/opencode-ref（opencode v2.0.24，TypeScript monorepo，tag e7a34f09）
> 移植实现：/opt/opencode-rust（14 workspace crates，版本 2.0.24）
> 方法：27 条逐行对比 lane 覆盖两侧全部实现源码（非抽查），双侧 file:line 证据，不采信仓库内已有审查/parity 文档；两道 oracle 一致性门（P1/FINAL）裁定矛盾并抽查 11/11 条关键结论全部成立。
> 分域明细（每域完整逐功能行、差异清单 D-n、风险 R-n、覆盖声明）见 `lanes/cmp-*.md`（32 份原始报告已归档）。

## 表 0 · 审查规模与总体判定

| 项 | ref (TS) | rust | 判定 |
|---|---|---|---|
| 移植相关源码规模（非测试） | ~455k LOC（core 65.7k + codemode 10.8k + ai 30.8k + tui 61.4k + cli 12.2k + client 16.8k + server 4.0k + plugin 3.1k + schema 5.7k + protocol 4.2k + util 4.2k + theme 1.6k） | ~505k src（+host.js 3.7k）+ tests ~75.6k | 规模同量级 |
| 总体移植判定 | — | — | **高度忠实**：核心域（工具/会话循环/压缩/事件存储/权限/凭证目录/CLI/ACP/schema/openapi）逐行级对齐，大量文案字节级 pin；无 P0 数据损坏级缺陷 |
| 高危发现 | — | — | 10 项（G-21..G-30，登记册见 REPORT.md） |
| 产品级功能缺口 | — | — | 4 类（媒体未接线、SystemOne/evaluation、cloudflare-ai-gateway 语义、workerd/simulation 剖面） |

## 表 1 · 架构级实现方式差异（结构性，非缺陷）

| # | 面 | ref 实现方式 | rust 实现方式 | 影响评估 |
|---|---|---|---|---|
| 1 | TUI 渲染 | SolidJS 响应式树 + @opentui/solid 光栅（targetFps 60） | 自绘 Frame/paint→ANSI（serialize_canonical），~100ms 泵循环；`*_solid.rs` 双实现收敛单 solver | 语义级等价，golden 帧字节级钉死；动画帧率上限不同 |
| 2 | CodeMode 解释器 | 进程内 acorn AST 白名单解释器（class/eval 结构性不可用） | bun guest（host.js）裸 JS 执行 + 静态扫描 import/require | **高危 G-01**：Bun/process/`__rawFetch` 全可达（沙箱逃逸面） |
| 3 | JS 插件运行时 | 进程内 import，effect+promise 双 API，每 Location 一套 PluginHost | oc-plugin-host bun 子进程 NDJSON JSON-RPC，全进程单 guest，仅 promise API | API 面 14/20 域对齐；两条生产断线（G-21/G-22） |
| 4 | 数据目录 | 原地 v1→v2 | XDG 根隔离 `opencode-rust` + 一次性单向 COPY（v2_migration.rs） | 与 TS 版可共存；源只读 |
| 5 | HTTP API/客户端 | effect HttpApi 手写 33 group / 生成 client 双面（effect+promise） | tools/openapi-gen 生成 routes.gen/validation.gen/methods.gen/types.gen + openapi.json（sha 钉死） | 141 operation 字节级 0 diff；rust 加 1 端点（plugin/install） |
| 6 | AI provider 栈 | @ai-sdk JS 包 + npm 动态安装桥（`aisdk:` 前缀） | oc-ai 原生协议状态机 18 族 + provider_catalog native 表 | npm-install 桥未移植（G-54）；协议原生实现自研且自证 |
| 7 | PTY 常驻终端 | 外部 opencode-pty 二进制（产物，源不在 repo） | oc-pty bin + 第一方状态机/wire（协议 v7 超集） | 高保真；版本号复用有互操作隐患（G-10） |
| 8 | 语法高亮 | web-tree-sitter wasm 运行时拉取（35 filetype） | tree-sitter 原生 vendor（默认 20 + feature 扩展） | 默认构建覆盖窄（G-51）；离线确定性为反向优势 |
| 9 | 升级通道 | opencode.ai/update/api + homebrew | 自身 GitHub release feed + sha256 + build-marker 校验 | 注册分歧（G-58）；默认服务端口刻意错开 |
| 10 | Solid 数据投影 | solid/data.ts：串行 revalidate + 逐事件失效 + optimistic outbox | store.rs reduce + 全量 resync 快照 + Last-Event-ID 游标 | 模型替换（G-38：6 类事件延迟到 resync） |

## 表 2 · 逐域功能对比（24 域；每行=一个审查域，详情见 lanes/cmp-*.md 的逐功能行）

| 域 | ref ↔ rust 规模 | 对齐度 | 代表性实现方式差异（完整 D-n 见明细） | 最高风险 |
|---|---|---|---|---|
| **内建工具**（read/edit/write/patch/glob/grep/shell/question/skill/subagent/webfetch/websearch/mcp-resource/opencode.*） | 4.25k+插件 ↔ tool/ ~12k | ✅ 高度对齐（多条字节级 pin） | 工具名两侧同为 shell/subagent（早前"bash/task 改名"说法不实）；输出 schema 全豁免（G-230 有意）；read AGENTS.md 注入用词法路径；websearch progress 时机 | G-17（低）：question 布尔回案；注册面无名称校验 |
| **会话循环**（runner step/drain/retry/publisher/model-request） | ~6k ↔ runner/ ~20k | ✅ 高度忠实（G 编号自证偏差注释体系） | 单写者事件循环替代 fiber 并发；aisdk JS 桥→原生协议；WS 会话通道参数全对齐 | **G-03（P1）**：中断后 hosted RESULT_MISSING 多发 tool.failed |
| **会话服务**（store/inbox/projector/fork/revert/move/transfer/subagent/title/stats/execution） | ~12k ↔ ~15k | ✅ 高保真 | G-226 合成失败行；60s/10s 有界等待；usage fan-out 串行化 | G-18（低） |
| **压缩与提示组装**（compaction/instructions/system-prompt/agent/variant） | ~4k ↔ ~6k | ✅ 高度忠实（文案逐句 verbatim） | plan 提醒不落库且渗入压缩请求；platform 值 macos/win32 | 无高危 |
| **配置/环境/项目**（config 发现/normalize/watch/v1/wellknown/workspace/project） | ~8k ↔ ~12k | ✅ 高保真（诊断消息级） | **mcp.timeout 文档级默认、snapshots、lsp 三键被接受但不生效**；数据根隔离迁移 | G-04（P1）：snapshots:false 失效（ref 侧确为开关） |
| **事件与存储**（bus/event/kv/database/48 迁移/repository） | ~14k ↔ ~18k | ✅ 高度忠实 | 52 会话事件 durable 分类 52/52 全对；48 迁移 ID 全同；19 终态表同名 | 无正确性缺陷（session.diff 为 rust 超集注册） |
| **快照/VCS/文件安全**（snapshot/vcs/git/file-access/mutation/retention/job） | ~2.5k ↔ ~5k | ✅ 高保真 | git2 直建影子库替代 CLI；vcs 命令无 cfg 标志+scope 放大+status 统计恒 0 | G-11/G-13（中低） |
| **MCP + 权限**（client/registry/oauth/elicitation/permission/saved/managed-policy） | ~4.5k ↔ ~12k | ✅/🟡 | 连接期静态 bearer 替代 OAuthClientProvider（运行期 401 不续期）；List* 不跟 nextCursor；工具桥不传 sessionID | G-07（分页截断）/G-08（token 过期无恢复）/G-14 |
| **凭证/集成/模型目录**（credential/oauth page/integration/models-dev/provider/resolver/copilot 目录/azure/modal/DO） | ~8k ↔ provider_catalog+integration ~15k | ✅ 高度忠实 | models.dev 快照 **md5 相同**；npm-install aisdk: 桥未移植 | **G-06（P1）**：默认模型无"首个可用 text 模型"回退 |
| **PTY/shell/文件系统**（pty/persistent-pty/shell 解析 OpaqueReason/formatter 26 内建/ripgrep/watcher/protected） | ~8k ↔ ~14k | ✅ 高度忠实（OpaqueReason 九值逐串、formatter 26 顺序钉死） | portable-pty/notify 替代平台绑定；fs/find 无 fff 后端贪心评分 | **G-02（高）**：非持久 pty per-chunk lossy UTF-8 损坏 CJK；G-10 协议超集 v7 |
| **核心杂项**（image/form/mime/location/instance/app/rpc/websearch 服务/skill 发现/worktree/reference） | ~7k ↔ ~10k | ✅ 质量高 | location 关闭缺 rpc.close；mime 三副本不一致；skill URL 拉取串行+30s 超时 | G-09（中） |
| **CodeMode**（interpreter/stdlib/limits/openapi/extension/tool-runtime + core execute/catalog） | 10.8k ↔ host.js 3.7k+seam | ❌ 宿主面✅/解释器语义换轴 | 白名单解释器→Bun 全集+逃逸面；签名恒 `Promise<void>`（ref `string\|null`）；诊断无 kind/位置；openapi/fromSpec 未移植 | **G-01（高）沙箱逃逸**+G-05（限位仅桥上） |
| **AI 生成管线**（llm/cache-policy/effort/tool-history/provider-error 34 正则/route 全链/WS 通道/schema 19 事件） | ~10k ↔ ~15k | ✅ 高对齐（27 项逐条） | 错误体 2KB 截断先于分类；Generation 轮询词汇分散进 media | **G-21（高）**：WebSocketSeams 构建后零消费 |
| **AI chat 协议族 1**（openai×4/anthropic/gemini/interactions/vertex + utils 13） | ~18k ↔ ~25k | ✅ 高度忠实 | gemini 命名空间重写丢 3 字段；interactions 事件零校验；vertex 守卫缺；/compact 宽容解析 | G-30/G-43/G-44/G-47 |
| **AI chat 协议族 2**（bedrock 全家/mistral/cohere/venice/zai/alibaba/meta/open-responses×3/xai/copilot/gateway） | ~15k ↔ ~20k | ✅ 高度忠实（sigv4 AWS 向量钉死；venice/zai/alibaba 并入 openai_chat overlay） | 诊断 body 缺失；AWS 凭证链子集（无 assume-role）；copilot bearer=refresh 已核实一致 | 低危簇 G-52 |
| **Provider 目录矩阵**（46 provider 定义/builtins 表 53/包映射） | 46 文件 ↔ native/facade/catalog | **31✅/13🟡/2❌** | ❌：openai-compatible-responses 自定义车道、typesafe-ai；🟡：媒体 9 家未接线+cloudflare-ai-gateway+alibaba region+zen evaluation | G-32/G-33/G-34（产品级缺口） |
| **AI 媒体**（image 9/speech 5/transcription 5/video 4 + 三家族 + route/fal_queue） | ~9k ↔ media/ ~15k | ✅ 高保真（23 协议逐一对应） | media-client/模型工厂未移植（自认 seam）；必填字段 unwrap_or_default 宽松 | G-28（runway cancel 404）/G-29 |
| **TUI 运行时/渲染/输入**（app/渲染/ANSI/颜色/键盘鼠标/剪贴板/attention/sound/editor/suspend/storage） | ~15k ↔ ~35k | ✅ 高保真（golden+逐字节测试钉死） | 剪贴板宿主写臂缺（仅 OSC52）；SIGHUP 无优雅收尾；boot 300ms 丢键窗 | **G-26（高）**：stdin 双读竞争 |
| **TUI 输入与对话框**（prompt 管线 13 文件/composer 四 tab/dialog 22 个/command palette） | ~12k ↔ ~20k | ✅ ~95% | 对象形式键绑定丢弃；附件标签复用判据/PDF 误标；命令面板静态表 | **G-27（高）**：pin/unpin 死键 |
| **TUI 消息渲染**（part 全类型/分组/虚拟化/diff-viewer/sidebar/tabs/stats/markdown/语法高亮） | ~20k ↔ ~25k | ✅ 高保真（+golden 独有设施） | 低 verbosity 包裹族；mount flat 权重；tok/s 口径；Diagnostics 行缺 | **G-25（高）**：locale_duration %10_000 bug |
| **TUI 配置/keymap/主题/mini/插件**（keybind 230 条/TuiConfig/33+1 主题/mini 38 文件/TUI 插件） | ~30k ↔ ~28k | ✅ 资产逐字节级（keybind 230/230 全对齐+8 补充；主题 md5 全等；ref 本身即 OKLCH） | tps 默认反转；win32 ctrl+z 反转；guest 无 markdown 渲染器注册 op | G-45/G-48 |
| **HTTP server**（auth/SSE/pairing/middleware/141 端点/handlers/进程生命周期） | ~8k ↔ ~33k | ✅ 141/141 端点（+1 rust 独有） | workerd/fetch 剖面缺失；simulation 显式拒绝；shell DELETE 幂等差；vcs.init 插件 provider 不可达 | G-35/G-36 |
| **client SDK**（service 生命周期/SSE/shared-events/rpc/solid 投影） | ~17k ↔ ~20k | ✅ 高保真 | resync 快照模型替代增量失效；effect 面缺失；RPC 订阅连接放大 | G-37/G-38 |
| **CLI**（命令树全量/run/noninteractive/services/updater/askpass） | ~12k ↔ ~19k | ✅ 命令树/flags 全对齐（run 事件流逐语义） | 升级通道换源；TTY 权限交互扩展；入口诊断信号缺席 | G-39（mini 无限重试） |
| **ACP**（20 文件：agent/turn/translate/permission/elicitation/replay/catalog） | 3.3k ↔ 7.9k | ✅ 结构逐文件对齐（wire v1 一致） | cancel/drain 语义；ask 阻塞折叠；MCP 重注册去重缺；child _meta 缺 | **G-23/G-24（高）**+G-40/41/42 |
| **插件宿主**（supervisor/ops 20 域/registrations/install/update/tap） | plugin 64 文件+core 基建 ↔ oc-plugin-host | 14/20 域 ops 全对齐 | rpc client 面缺；vcs/worktree 读面缺；事件 best-effort；install 无原子性 | **G-21/G-22（高）**+G-46 |
| **schema/protocol/theme/util**（session-message parts/30 错误类/patch 算法/theme 实现/util 全家） | ~12k ↔ ~14k | ✅ 很高（openapi 字节级 0 diff；patch 逐行等价） | simulation 协议未移植；flock 简化；BTreeMap 键序；identifier 竞窗 | 低危簇 G-61 |

## 表 3 · 配置/键面/资产级精确对照（摘要）

| 面 | 结果 |
|---|---|
| 顶层 config 键 | 1:1；例外：`mcp.timeout` 文档级默认不生效（per-server 生效）、`snapshots` 不生效、`lsp` 惰性、rust 独有 `experimental.file_retention.cleanup_interval` |
| TUI keybind | ref 230 条在 rust 全部同名同默认值；rust 另有 8 条补充（ref 为组件内直绑）；leader ctrl+x/2000ms 一致 |
| TuiConfig 键 | 全键可解析；`session.tps` 默认反转（ref true/rust false）；少数键解析后消费点待确认 |
| 主题资产 | 33 v1 + v2 逐文件 **md5 全等**；token schema 全实现；色彩空间两侧同为 OKLCH |
| bus 事件 | 会话域 52/52 分类+durable 版本一致；非会话域 6 类由 server ad-hoc 发布（行为等价） |
| DB 迁移 | 48/48 ID 一致；19 终态表同名同索引 |
| 模型目录快照 | models-dev snapshot.txt **md5 相同**（5,316,382B）；224 providers/8380 models 锚定 |
| openapi.json | 141 共有 operation 逐对象 sort-keys **字节级 0 diff**；rust 多 1 path（plugin/install） |
| session-message parts | 11 变体/AssistantContent/ToolState/Compaction 双 tag 全集无缺漏，round-trip wire 测试钉死 |
| CLI 命令树 | 全部命令/子命令/flag 对齐；rust 加 completion/--version-rust/--force；--standalone 全局化（超集） |

## 表 4 · 未移植/范围外清单

| 项 | 状态 | 说明 |
|---|---|---|
| 媒体 provider 部署接线（13+ 家 image/speech/transcription/video） | 协议已移植、**未接线** | oc-core/oc-server 对 oc_ai::media 零引用（G-32） |
| SystemOne/evaluation 域 | ❌ 缺失 | typesafe-ai provider、zen/openrouter/vercel evaluation 车道（G-33） |
| cloudflare-ai-gateway 网关语义 | ❌ 降级 | 仅按显式 baseURL 的 plain openai-chat（G-34） |
| workerd/fetch 运行时剖面 + simulation 协议 | ❌ 范围外/拒绝 | 独立进程 axum 形态；OPENCODE_SIMULATE 显式拒绝 |
| codemode openapi/fromSpec、Extension 通用注入、Storybook | ❌ | 库 API 面/开发设施 |
| npm-install aisdk: 动态 provider 桥 | ❌ | 非 builtin npm 包不可用 |
| ref 其余包（app/console/desktop/web/gui-extensions/session-ui/ui/sdks/merman/stats/mermaid-as-service 等） | 范围外 | 桌面/Web 控制台/SDK 生态不在移植范围（mermaid/latex 渲染反为 rust TUI 独有） |
| rust 独有超集 | ➕ | golden 测试体系、oc-pty 守护第一方化、replay harness、attention.bell、plugin/install 端点、mermaid/latex 渲染器、crash-restore、Last-Event-ID 游标、safekill/pidfd、build-marker 校验 |

---
完整证据链：`lanes/`（inv-×3 清单、cmp-×26 分域报告、gate-P1/gate-FINAL 两道门）；各 cmp 文件含逐功能行、D-n 差异清单（双侧 file:line）、R-n 风险、覆盖声明（诚实列出未逐行部分）。生成日期：2026-11-10。
