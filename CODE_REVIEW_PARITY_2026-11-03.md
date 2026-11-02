# OpenCode Rust 移植 vs TS 参考实现 — 全功能逐项对比审查总报告

- 审查日期：2026-11-03
- 参考实现：/opt/opencode-ref（anchor 版本）
- 移植实现：/opt/opencode-rust
- 方法：22 条并行审查通道，覆盖 core/tui/cli/client/server/schema/theme/plugin/util/ai/codemode/mermaid 全部在端口范围内的包；每条通道被要求**逐文件完整阅读两侧源码（禁止抽查）**、只以代码为准（忽略仓库内已有 .md 审查文档）、按统一格式产出「覆盖清单 + 对比表 + Top 缺口」。各通道报告见本目录（域报告 22 份）。
- 规模：TS 侧在端口范围内约 21 万行全读；Rust 侧对应约 40 万行（非测试生产代码全读，测试文件粗读佐证）。
- 状态图例：✅等价 / ⚠️部分 / ❌缺失 / ➕Rust新增 / 🔁方式不同但行为等价

---

## 一、域级总对比表

| # | 域 | TS 侧 | Rust 侧 | 总体状态 | 一句话结论 |
|---|---|---|---|---|---|
| 1 | Session 会话引擎 | core/session + session.ts (~10.5k) | oc-core/session (29k) | ✅(高保真) | 生成循环/压缩/重试/投影/事件发布逐行对齐；缺口：transfer export/sanitize 全缺、compaction due() 边界、native 压缩 2 处偏差 |
| 2 | 内置工具集 | tool.ts + tool/plugin (4.6k) | oc-core/tool (19.7k) | ✅ | 6 文件系统工具字节级等价；缺口：subagent/mcp-resource 绕过执行管道(无 hook/decode/normalize)、websearch Disabled hook、shell metadata 字段 |
| 3 | AI 协议/流处理 | packages/ai protocols+route+schema (~20k) | oc-ai 对应 (~30k) | ✅ | SSE 机/usage 双向/reasoning 签名/错误正则逐行等价+50 盒对拍；缺口：ToolSchemaProjection 未中央化、experimental/ 缺 |
| 4 | AI Provider 绑定 | packages/ai providers (~12k) | facade/native (~10k) | ⚠️ | 协议层高保真但 **facade 绑定层多 处脱线：Azure 整体未接线(死代码)、第三方 anthropic 根 URL 拼错、MiniMax/Meta messages 专用协议缺、compat preset 注入不全** |
| 5 | 插件系统 | core/plugin + packages/plugin (~13.3k) | oc-plugin-host (~15.7k) | ⚠️ | 架构换为 bun 子进程+NDJSON(语义大部分等价)；缺口：Effect API 不支持、内建行为插件成批缺失(plan/identity/verbosity/optimize/init/review/hg)、TUI 插件面收窄、多 location 串扰 |
| 6 | MCP+凭证+模型目录+Copilot | mcp/models-dev/credential/oauth/copilot (~9k) | oc-core/mcp+provider_catalog+integration | ✅/⚠️ | MCP 全链与 models.dev 226 家对拍一致；**Copilot 域多处缺口：/v1/messages 未接线、动态模型同步死代码、limits/cost/variants 派生缺、免费标题通道缺** |
| 7 | Shell/PTY/进程/任务 | shell/pty/persistent-pty/environment/job (~9.3k) | oc-core/shell+oc-pty+persistent_pty (~22k) | ✅ | 5355 行词法器逐语句翻译；➕完整 Rust 守护端(含 checkpoint)；缺口：注释后换行扫描偏差(**安全**)、$NAME 字符集、ripgrep.find、PS 续行 save 串 |
| 8 | 配置/指令/skill/agent | config+instructions+skill+command+agent (~5.1k) | oc-core/config+cli_config (~13k) | ✅ | 六源合并/40 项 v1 迁移/38 字段全对齐；缺口：~/.claude/skills 兼容、workspace 注册表、无热重载、MCP timeout 合并 |
| 9 | Schema/协议类型 | packages/schema+protocol (~10k) | oc-schema+生成代码 | ✅ | 141 端点 byte-identical、28 项 discriminated union 对齐；**Mcp.Protocol 变体名大写=wire bug**、v1 legacy 类型缺 |
| 10 | 事件总线/数据库 | bus+database+event (~8.5k) | oc-core/bus+database (~13k) | ✅ | 48 条迁移逐条对齐、总线不变量等价；缺口：session.diff 过度公开、log 流有界 64 静默断流、publishAll 时间戳共享 |
| 11 | 文件系统/VCS/快照 | filesystem/git/vcs/worktree/snapshot (~4k) | filesystem+file_*+snapshot+project | ✅/⚠️ | 访问模型/变更追踪等价；缺口：clone 无 --depth、快照 diff 无二进制检测/10MB 截断、VCS 无注册表/BranchUpdated、gitExecutable 硬编码 |
| 12 | HTTP Server | packages/server (~4.1k) | oc-server (45.6k) | ✅ | 142 路由 1:1、中间件/SSE/PTY 全等价；缺口：workerd/fetch 模式、options 面、若干文案 drift |
| 13 | Client SDK | packages/client (~17k) | oc-client (19.4k) | ✅ | 139/139 方法对齐；缺口：~30 端点缺 404 declared、无 per-request RequestOptions、无 is*Error 守卫 |
| 14 | CLI | packages/cli (~14.3k) | oc-cli (28.9k) | ✅ | 命令树/ACP 13 方法逐项等价；缺口：v2 更新服务流换轨、heap/cpu-profile、run/v1 桥、启动 seam 未接线 |
| 15 | Theme+Util+语法 | theme+util (~5.9k) | oc-theme+oc-util+oc-syntax | ✅ | 34 内置主题 sha256 字节级一致、47 条语法规则对齐；缺口：generateSystem、flock 协议降级、npm check/update、artifact.ts |
| 16 | Codemode 沙箱 | core/codemode+packages/codemode (~11.5k) | tool/codemode.rs+host.js | 🔁/⚠️ | 外围高保真；核心换成 bun guest 真引擎：**沙箱禁足失效(fs/import/timer 可达)、console 直写协议通道、fetch 无约束、嵌套 files 收集缺** |
| 17 | core 杂项(权限/表单/集成/位置) | permission/form/integration/location 等 (~5k) | permission/forms/integration/location_services | ✅ | 权限状态机/表单校验/集成 attempt 机逐条对齐；缺口：Generate.text、LocationActivity 驱逐语义、Modal 发现、Instance 图缩减 |
| 18 | TUI 骨架/路由/store | tui 根+routes+context+config (~16k) | oc-tui app/store/keymap 等 | ✅ | **241 条默认键位字节级一致**、reducer/滚动/tabs 全对齐；缺口：剪贴板 host 写臂、启动移交、IDE mention 监听、location 维度塌缩 |
| 19 | TUI 组件-消息/编辑器 | tui component+prompt (~18k) | oc-tui components (~15k 对应) | ✅/⚠️ | 渲染表面字节级；缺口：Markdown 表格不渲染、消息内 diff 降级、多文件 patch 只显示 1 个、Question 块缺、后台 shell 不流式、slash 表硬编码 |
| 20 | TUI 组件-导航/框架 | tui component/ui/util (~18.7k) | oc-tui components/footer 等 | ✅ | fuzzysort 位级、八分块 spinner、footer 全家族；缺口：DialogSelect details/locked/tab-actions、rename 擦除动画 |
| 21 | Mini 模式 | tui/mini (~18.2k) | oc-tui/mini (8.2k) | ⚠️ | 骨架 1:1 但**约 60% 功能缺：富文本渲染管线、面板族(agent/model/variant/queued/settings)、事件面、断线重放、/new、主题探测、SIGWINCH** |
| 22 | Mermaid/特性插件/图片 | merman+feature-plugins+plugin (~19k) | oc-tui/render 等 (~30k) | ✅/⚠️ | 4 种图全链+声音/通知等价；缺口：**flowchart 汇流排分组键 bug**、`---` 开放边画箭头、流式截断恢复、generateSystem、iTerm2 协议 |

范围外（未列入端口范围，Rust 无对应属预期）：web/console/desktop/app/session-ui/ui/sdks/stats 包、workerd 运行时剖面、多运行时 SQLite 适配层。

---

## 二、全局 Top 缺口（跨通道汇总，按严重度）

### A. 功能性错误（wire/行为 bug，建议优先修）
1. **Azure provider 未接线**（#4）：oc-ai/protocols/azure.rs 完整移植却是死代码；catalog `azure` 走通用 OpenAIResponses 族 → 认证/URL/头全错，未配 baseURL 时静默错发 api.openai.com。
2. **Mcp.Protocol serde 变体名大写**（#9，mcp.rs:26 无 rename_all）：wire 输出 `"Legacy"/"Auto"`，v2 配置 `"protocol":"auto"` 解析失败。
3. **Copilot /v1/messages 车道未接线**（#6）：advertise anthropic 端点的 Claude 模型会被错发 chat/completions。
4. **第三方 anthropic 根 URL 拼错**（#4）：moonshot `/v1/messages`≠`/anthropic/v1/messages`、zai-coding-plan、alibaba 同类。
5. **flowchart 汇流排路由分组键 bug**（#22，routing.rs:898 source_key/target_key=edge_index）：fan-out/fan-in 分组整体失效。
6. **shell 权限扫描：注释后换行被跳过**（#7，scan.rs:1846）：`cmd1 # c\ncmd2` 合并为单条命令，cmd2 逃避独立权限分类（安全向）。
7. **TUI Markdown 表格不渲染**（#19）：含表格回复直接丢结构。
8. **session.diff 被过度公开到 SSE**（#10）：ref 中为 v1 legacy 不转发。
9. **`A --- B` 开放边仍画箭头**（#22）。
10. **bus log() 流有界 64，慢消费者静默断流**（#10）。

### B. 安全/沙箱语义弱化
11. **Codemode guest 内 fs/require/import/timer/Bun 全可达**（#16）：与 execute 工具描述文本相悖；console 直写 NDJSON 协议通道；fetch 无 30s 超时/大小上限。
12. 守护 PTY 二进制无 sha256/属主校验链（#7）；html-markdown 命名实体表仅 ~60（#2）。

### C. 功能缺失（成块）
13. **Mini 模式约 60% 缺失**（#21）：富文本渲染、模型/变体切换面板（model_pref.rs 是死代码）、inbox/compaction/skill/usage 事件面、断线 hydrate 重放、/new、主题调色板探测、SIGWINCH。
14. **插件生态面**（#5）：Effect 插件 API 全不支持；plan/identity/verbosity/optimize/init/review 命令/opencode+report 技能/Mercurial 内建缺失；TUI 插件 API 收窄；动态 npm provider 工厂缺；aisdk language 句柄不可替换。
15. **Copilot 目录**（#6）：动态 /models 同步无生产调用点；limits/modalities/cost/variants 派生缺；gpt-5-chat-latest 未隐藏；免费标题通道缺。
16. **session transfer export/sanitize 全缺**（#1）；stateless Generate.text 缺（#17）；Modal 动态模型发现缺。
17. **TUI 展示缺口**（#19）：消息内 diff 仅 unified 无高亮/split；多文件 patch 只渲染第一个；Question 工具块缺；工具输出图片缺；后台 shell 输出无流式拉取；prompt stash 缺、history 降级纯文本；slash 命令表硬编码（插件/服务器自定义命令不可见）。
18. **工具管道**（#2）：subagent/mcp-resource executor 绕过 PipelineExecutor（无 hook/decodeInput/normalizeContent）；websearch Disabled 不移除工具、Form 缺 kind 元数据；shell metadata 缺 signal/timeout。
19. **Provider 语义散失**（#4）：gpt-5 默认 reasoning 选项缺；OpenAI chat prompt_cache_key 丢弃；MiniMax thinking/reasoning_split、Meta messages 协议缺；requireSignature 靠启发式；Vercel Gateway env 回退失效；ToolSchemaProjection 未中央化（#3）。
20. **CLI/宿主互操作**（#14/#18）：v2 更新服务流换轨自有 feed；剪贴板 host 写臂缺；启动移交 openSessionHandoff/terminalHandoff 缺；IDE @mention 监听缺；heap/cpu-profile 诊断缺。
21. **其余**：ripgrep.find、generateSystem 系统主题、v1 legacy schema 类型、workerd/fetch 嵌入模式、experimental/、~/.claude/skills 兼容、clone --depth、快照 diff 二进制检测、Npm.check/update、observability 日志压缩、artifact.ts。

### D. 低风险语义偏差（择要）
- compaction due() 边界、native 压缩 webSocket:false/finish=error 不重试、toSessionError 覆盖窄（#1）
- promptCacheKey 大写 hex、publishAll 批内 created 共享、ephemeral data 必须 object（#1/#10）
- PS 反引号续行/UTF-16 save 边界 → 授权串失稳（#7）
- 客户端 404 declared 缺 ~30 端点、无 RequestOptions/is*Error（#13）
- config 非法值静默回退（TS 报错）、无热重载（#8/#18）
- LocationActivity 误驱逐 live 位置（#17）

### E. Rust 有意新增（升级 v2 时需同步）
- 持久 PTY 完整守护端（checkpoint v1、owner superseded、orphan grace）
- cassette 对拍测试体系、golden 测试、safekill 世代校验
- OS 通知(OSC9/777/99)、attention.bell、toast.activate 键位、tabs close-hold/drag
- `POST /api/plugin/install` 新路由、v2_migration 一次性拷贝迁移、WS proxy.rs
- run 非 TTY 权限交互提示（ref 恒自动拒）

---

## 三、总体判断

核心引擎层（session 生成循环、工具执行、事件总线/数据库迁移、shell 词法器、AI 协议流处理、schema/路由、TUI 键位与 reducer、主题字节）达到**逐行乃至字节级等价**，且大量自带对拍测试钉住 ref 语义；行数膨胀 ~40% 主要来自测试、守护端与 Rust 惯用展开。

主要欠账集中在四个面：
1. **Provider facade 绑定层**（Azure 脱线、URL 根、compat preset）——用户可见故障；
2. **Mini 模式与 TUI 富文本/动态数据源**（表格、diff、面板、流式）——体验差距；
3. **插件生态与 Copilot 目录**（Effect API、内建行为插件、动态模型）——生态能力；
4. **长尾工具与宿主互操作**（transfer export、剪贴板、启动移交、更新服务）。

各域详细逐项对比表与行号级证据见同目录 22 份域报告：
session.md / tools.md / ai_protocols.md / ai_providers.md / plugin.md / mcp_auth.md / shell_pty.md / config.md / schema.md / bus_db.md / fsvcs.md / server.md / client.md / cli.md / theme_util.md / codemode.md / core_misc.md / tui_app.md / tui_components_a.md / tui_components_b.md / tui_mini.md / render_mermaid.md
