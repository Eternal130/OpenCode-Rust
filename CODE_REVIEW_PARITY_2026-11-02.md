# OpenCode Rust 移植逐行对比审查（2026-11-02）

锚定参考：`/opt/opencode-ref`（TS）。本审查逐包、逐文件、逐行比对两侧实现，**未采信仓库内任何已有审查/迁移文档**。
各包明细表（功能项 × TS 位置 × Rust 位置 × 状态 × 差异说明）见 `/tmp/opencode/review/*.md`。

## 一、总对比表（按包）

| 包（TS → Rust） | 功能项 | ✅ 一致 | ⚠️ 部分 | ❌ 缺失 | ➕ Rust 额外 | 明细 |
|---|---|---|---|---|---|---|
| util/theme/schema/protocol → oc-util/oc-theme/oc-schema/oc-syntax | 106 | 58 | 27 | 9 | 12 | util_theme_schema.md |
| ai → oc-ai | 78 | 65 | 5 | 2 | 5 | ai.md |
| core 主链路（session/tool/agent/command）→ oc-core | 60 | 55 | 13 | 0 | 2 | core_a.md |
| core 基础设施（db/mcp/fs/perm/snapshot 等）→ oc-core | 124 | 83 | 31 | 7 | 3 | core_b.md |
| server → oc-server + oc-http | 53 | 41 | 10 | 1 | 6 | server.md |
| client/sdk → oc-client | 33 | 19 | 6 | 8 | 4 | client_sdk.md |
| tui 渲染/组件/编辑器 → oc-tui(A)/oc-tui-core/oc-syntax | 88 | 62 | 17 | 7 | 2 | tui_a.md |
| tui 应用/路由/mini/插件 → oc-tui(B) | 96 | 72 | 12 | 2 | 5 | tui_b.md |
| cli/plugin → oc-cli/oc-plugin-host/oc-pty | 70 | 51 | 15 | 4 | 7 | cli_plugin.md |
| **合计** | **708** | **506 (71%)** | **136 (19%)** | **40 (6%)** | **46 (6%)** | |

覆盖率：TS 侧全部纳入映射；Rust 侧除 core-B 约 1 万行大型实现体（rmcp 内部、v1 迁移映射、PTY daemon 状态机、provider catalog normalize 等，已完成 TS 全文 + Rust API/常量/错误/测试比对，标注 [S]）与 tui-B 两侧测试模块外，其余均双侧逐行通读。

## 二、正确性最扎实、可直接信任的部分

1. **HTTP 面 100% 对齐**：141/141 端点 method+path 一致；鉴权逐语义复刻（含常时比较、sec-fetch-mode 门控、豁免顺序），中间件顺序正确。
2. **数据库层最高保真**：48 个迁移、DDL/索引、0600 权限、按列名解码。
3. **权限决策逐语义等价**，有对照测试钉住。
4. **step 终局 13 步顺序、幂等/并发语义**（inbox first-admission-wins、job 幂等 join、title 乐观并发）1:1。
5. **流式解析**：Rust parity 测试直接回放 TS recordings cassettes。
6. **TUI 键位表约 250 条默认值零漂移**；主题/音频资产字节级相同；剪贴板/注意力/音频平台层完全对齐。
7. **oc-schema 高风险语义点**（UTF-16 哈希单位、OKLCH 逐位系数、51 个事件变体、30 个配置字段）全部正确。

## 三、必须修复（正确性/数据错误/panic）

| # | 问题 | 位置 |
|---|---|---|
| 1 | DESCRIPTION_LIMIT 按字节切片，多字节字符边界直接 **panic**（TS 按 UTF-16） | oc-core codemode catalog |
| 2 | session.move subpath 复用源位置 `row.path` 而非目的地相对路径——**跨项目移动写入错误数据** | oc-core move.rs |
| 3 | pty cursor 单位字节 vs UTF-16 码元；订阅回调 panic 毒化服务锁 | oc-core/oc-pty |
| 4 | DigitalOcean `cache_control` ext 漏配 → **提示缓存整体失效** | oc-ai openai_chat.rs:3178 等 |
| 5 | MCP OAuth `resource` 参数无 scheme，违反 RFC 8707；`iss` 不校验；saved-grant DB 错误被吞（授权无感知失效） | oc-core oauth.rs:765 / permission/sql.rs |
| 6 | `auth import` 任何失败都计为 skip 且 exit 0（吞网络/5xx 错误） | oc-cli |
| 7 | `session.update` 空标题不触发 LLM 重生成（行为与 v2 相反，自认 TODO phase-9） | oc-server session.rs:546 |
| 8 | 触发检测/emoji 宽度用字节或 char 而非显示宽度——宽字符混排锚点/行宽偏差 | oc-tui |

## 四、成片功能缺失（用户可感知，按影响排序）

| # | 缺失功能 | 来源 |
|---|---|---|
| 1 | TUI `@`-文件模糊搜索与实时目录补全（自认 "async half, not ported"） | tui_a N3 |
| 2 | mini/tool.ts（1358 行）结构化工具快照渲染 | tui_b |
| 3 | createAnimatable 动画引擎（spring/tween）；相关 fade/resize 动画静态化 | tui_a |
| 4 | Formatter 为 no-op——写入后不格式化 | core_b |
| 5 | 插件 API 缺 `ctx.model` 域、`ctx.plugin.list`、effect 风格插件 API | cli_plugin |
| 6 | git clone/fetch/checkout/resetHard 与 vcs.ts/patch.ts（worktree CRUD 已移植） | core_b |
| 7 | 服务生命周期整套（discover/ensure/stop、contender 池、退避、SIGKILL）与 pty-handoff sidecar | client_sdk |
| 8 | AbortSignal 取消全链路（TS 每个方法/SSE/RPC 均支持） | client_sdk |
| 9 | `auth login` 交互表单系统（when 条件/校验/--answer 类型） | cli_plugin |
| 10 | run v1 兼容层、根命令 10min updater 循环与 TUI 接线 | cli_plugin |
| 11 | stats 的 GitHub 式日历网格/颜色/本地时区（Rust 为 UTC 纯文本）；updater 缺 npm/pnpm/bun/yarn/vp 检测；uninstall 不执行卸载 | cli_plugin 二轮 |
| 12 | PTY 终端主题注入（OSC 4/10/11）；导出选项交互表单；cliErrorMessage 友好格式化；openUrl 无 macOS/Windows 分支且丢 scheme 校验 | tui_a 二轮 |
| 13 | experimental/evaluation + system-one（~640 行）；media generation `events()` 进度流 | ai |
| 14 | tui-B：`session.toggle.thinking/scrollbar` 键位注册但无 dispatcher 臂；ctrl+shift+tab 在 crosserm 翻译层丢失（`session.tab.previous` 实机失效）；model favorite 写路径缺失（ctrl+f 永不持久化） | tui_b 二轮 |
| 15 | schema：rpc.ts、legacy-event、durable-event-manifest、models-dev 管道、v1 遗留 schema | util_theme_schema |
| 16 | client：RPC 定义层（141 方法表、事件过滤、`on()`）、sdk 包、service/effect/solid 变体 | client_sdk |

## 五、系统性模式（修复优先级建议）

1. **"命令注册齐全但 dispatcher 缺臂/分支不全"**：建议对 command_palette.rs 分发表与 TS index.tsx 命令表做一次机械对账（tui-B 补审结论）。
2. **"cassette parity 覆盖不到目录装配层"**：provider env 回退链/默认 baseURL/compat 标志位于录制盲区（togetherai/cerebras 默认 baseURL、deepinfra 全线、cerebras reasoningField 等），需补目录层对照测试。
3. **"宽容解析边界"**：untagged `Disabled(true)`、未知 hue step、v1 主题 null 静默跳过等——Rust 多处比 TS 更宽容，可能掩盖用户配置错误。
4. **有意增强（应记录而非修复）**：proxy.rs/bedrock 凭证链、replay+golden 测试、safekill、TOOL_JOIN_TIMEOUT、boot 时间戳护栏、`env:<port>` 编辑器连接等。

## 六、总评

移植整体质量显著高于典型移植项目：核心语义（消息生命周期、权限、数据库、HTTP、鉴权、键位、流式协议）逐行等价甚至有超集加固；约 19% 的 ⚠️ 项多为文案/可观测性/边界简化。真正风险集中在第三节的 8 项正确性缺陷与第四节 1–6 项成片缺失。
