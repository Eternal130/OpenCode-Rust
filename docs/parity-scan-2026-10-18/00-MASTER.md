# opencode-rust ↔ opencode v2.0.24 全量功能一致性审计（总报告）

- **参考实现**：`/opt/opencode-ref`，opencode v2.0.24，commit `e7a34f09bf`（TypeScript / bun）
- **目标实现**：`/opt/opencode-rust`，workspace `2.0.24`，审计基线 `bb364b6`
- **方法**：第一轮 11 条并行只读审计 + 第二轮 5 条补漏通道（HTTP 字段矩阵、config/env/policy、TUI 深扫、事件 payload、Copilot/媒体）。按子系统做「文件映射 → 逐语义对比 → 调用点 grep 验证 → 既有登记项复核」。所有差异均带双侧 `path:line`。
- **产物**：本总报告 + 16 份明细报告（`01`–`16`）。本报告负责去重、跨 lane 纠错、严重度统一与修复排序；**逐条证据以明细报告为准**。
- **版本**：2026-10-18 第二轮补扫合并版。第一轮结论保留；第二轮新增项集中在 §7。
- **重要边界**：静态审计，未跑 `cargo build/test`、未跑双端 live 差分。标注 `UNVERIFIED` 的条目需运行时复核；`B-3` 为静态推断。

---

## 0. 执行摘要

### 0.1 一句话结论

**引擎主干（drain/step/retry/compaction/event-sourcing/DB schema/HTTP 路由/主题资产）已高度对齐；真正的缺口集中在「最后一公里接线」和「外围用户表面」：凭据→请求、插件 guest→引擎、后台任务→父会话、DB 生命周期事件→消费者、生成客户端→服务器响应形状，以及 TUI/CLI/插件生态的若干整面缺失。**

### 0.2 规模与计数

| 通道 | 子系统 | 条目 | S0 | S1 | S2/其它 | NEW | KNOWN |
|---|---|---:|---:|---:|---:|---:|---:|
| 01 | 会话生命周期 / runner | 34 | 0 | 7 | 27 | 25 | 9 |
| 02 | 工具 / 执行运行时 | 33 | 0 | 8+ | 22 | 27 | 6 |
| 03 | AI 协议 / 传输 / 媒体 | 26 | 0 | 12 | 14 | ~18 | ~8 |
| 04 | provider 目录 / 鉴权 / 账号 | 22 | **3** | 9 | 10 | 18 | 4 |
| 05 | HTTP API / client / SDK | 22 | 0 | 4 | 18 | 14 | 8 |
| 06 | TUI | 34 | **2** | 15 | 17 | 15 | 19 |
| 07 | CLI / config / schema / 打包 | 20 | 0 | 6 | 14 | 6 | 14 |
| 08 | 插件宿主 / MCP / 权限 / 表单 | 20 | 0 | 11 | 9 | 16 | 4 |
| 09 | 数据库 / project / worktree | 20 | 0 | 6 | 14 | 18 | 2 |
| 10 | 包级表面 / 入口 / 平台 | 21 | 2(P0) | — | 19 | — | — |
| 11 | core 杂项（VCS/reference/image/…） | 26 | 0 | 4 | 22 | ~20 | ~6 |
| 12–16 | 第二轮补扫（HTTP 字段 / config-env-policy / TUI 深扫 / 事件 payload / Copilot-media） | **108** | **1** | **≈25** | **≈82** | **≈90** | **≈18** |
| **合计** | | **386** | **6** | **≈105** | **≈272** | **≈270** | **≈93** |

> 计数含跨 lane 重复（同一根因在多个子系统各记一次）与同一根因的多个 ID；去重后约 **340 个独立差异**。`KNOWN` 表示已有文档登记，不代表已修复。第二轮 108 条含 13 条 S3 文案/信封细节与 1 条 INFO。

### 0.3 六个 S0（主流程断裂）

1. **凭据最后一公里未打通**：`auth login` 得到的 key/OAuth 不进入模型解析与请求鉴权（仅 copilot/vertex/console 特例）——登录成功但请求仍按匿名/无 key 发出。`04:B-1`
2. **console 颁发 provider 无 token**：`api_key=None` 导致匿名请求。`04:B-2`
3. **keyless zen 无 `apiKey:"public"`/Auth.none 降级**：开箱默认模型请求期鉴权失败（静态推断，需运行时复现）。`04:B-3`
4. **插件 TUI UI slot 全缺**：`ctx.ui.slot/router.navigate/keymap.layer` 未实现，omo-slim 类旗舰插件的侧栏/交互完全不渲染。`06:#1`
5. **插件注册的 MCP 服务器不可见**：快照收集了 `mcps`，引擎/`DialogMcp` 不消费。`06:#2`, `08:D-33`
6. **主 TUI 表单阻塞面整体缺失**：`form.created` 只进 footer 队列，`app.rs` 的 frame/press 均无 `FooterView::Form` 分支；表单不可见、不可答、composer 照常可打字，回合假死。`14:TD-16`（第二轮新增，建议按 S0）

### 0.4 最接近 S0 的六项（建议按 S0 处理）

| 项 | 后果 | 证据 |
|---|---|---|
| 后台 subagent 完成不 wake 父会话 | 后台任务完成但父会话永不反应，直到用户再发消息 | `01:S-01` |
| 崩溃/重启恢复 sweep 未接线 | kill -9/重启后孤儿 turn 静默消失、后台通知丢失 | `01:L-01` |
| HTTP 超时预算全缺 | 挂起的响应头/停滞 chunk 无限等待（v2 默认 300s 自报错） | `03:5.1`, `04:B-6` |
| 普通 PTY WS meta 帧三重偏差 | web/desktop 断线游标恢复失效 | `05:F1` |
| VCS 三个端点线格式不符 schema | 强类型客户端（含 Rust TUI 自身）解码失败，diff/branch 视图不可用 | `11:#1-3` |
| shell stdout/stderr 顺序读取 | 交错丢失；stderr>64KB 时互锁到超时误报 | `02:C1` |

---

## 1. S0 明细

### S0-1 存储凭据不进入解析/鉴权/可用性（NEW）

- **EXPECTED**：`ModelResolver` 在解析 provider 时经 `Credential`/integration host 取「当前 active credential」，注入 baseURL/headers/apiKey；`/api/model` 的可用性折叠也读凭据（`packages/core/src/model-resolver.ts:364-371`, `provider.ts:393-406`）。
- **ACTUAL**：`engine/resolver.rs:69-116` 只读 env/config；`handlers/model.rs:92-93` 只看 env var。仅 `copilot.rs:108-119` 与 vertex 有显式 `CredentialBindings` 特例。
- **影响**：`opencode auth login` 对绝大多数 provider 无效；用户登录后仍得到 `ProviderAuthError`/401。这是「凭据 → 解析 → 请求」通用化缺失，属于一次通用改动可覆盖三个 S0 的根因。

### S0-2 console 颁发 provider 匿名请求（NEW）

- `console.rs:210` 构造 provider 时 `api_key=None`；`facade.rs:569-576` 未走 `integrationID="opencode"` 的凭据解析（ref `opencode.ts:251,547-552`）。

### S0-3 keyless zen 无 public key 降级（NEW，待运行时复现）

- ref `opencode.ts:317-325` 对 keyless zen 免费层注入 `apiKey:"public"` 或 `Auth.none`；Rust `facade.rs:476-485`, `auth.rs:101-112` 无该降级，静态路径会走鉴权失败。

### S0-4 插件 TUI UI 面缺失（KNOWN，rescan F1/F7）

- ref `ctx.ui.slot({append:"sidebar.content", render})`、`ui.router.navigate`、`keymap.layer`（`packages/tui/src/plugin/api.tsx:298-312,179-199`）。
- Rust `host.js:698-784` 只有 toast / 文本行 sidebar / footer hint / dialog；无 slot / router / keymap。
- 影响：omo-slim 等以 TUI 插件形态交付的旗舰插件侧栏、交互、子代理列表全部不出现。

### S0-5 插件 MCP 不可见（KNOWN，rescan F2）

- 插件快照 `registrations.rs:309-314` 收集 `mcps`，但 `oc-server/src/plugins.rs:241-269` 的 `HostState.mcps` 只含 config + codemode 默认表；`McpRegistry.config_source`、`LocationPluginSource` 均不合并快照。
- 影响：插件注册/改写的 MCP server 只存在于 guest 内 `ctx.mcp.list` 展示，不建立连接、不进 `/mcps`。

### S0-6 主 TUI 表单阻塞面缺失（NEW，第二轮）

- **EXPECTED**：`form.created` 在会话路由挂起一个阻塞表单（footer 扩展/全屏），键位接管、composer 禁用，`form.replied/cancelled` 后恢复。ref `form.ts` + TUI form 挂载点。
- **ACTUAL**：`store.rs:6708-6719` 只把表单推进 `footer.form_created`；`app.rs` 全文只有 `FooterView::Permission` 分支（`app.rs:1361/1386/1417/1528/1546/2087`），没有 `FooterView::Form` 的 frame/press 分支。`render_form_body` 只被 mini 与 golden 测试调用。
- **影响**：MCP elicitation / question 表单出现时主 TUI 完全不可见、不可答；composer 仍可输入，回合表现为假死。这是第二轮最重要的新增 S0。
- **关联**：`14:TD-17` 的字段类型缺失会让含 `boolean/number/integer` 的表单在反序列化阶段被静默丢弃，进一步放大该问题。

---

## 2. S1 明细（按子系统）

### 2.1 会话 / runner（7）

| ID | 问题 | ref → rust | 影响 |
|---|---|---|---|
| S-01 | 后台 subagent 完成不 wake 父会话 | `subagent-completion.ts:31-43`（经 `Session.synthetic` → `execution.wake`）→ `session/subagent_completion.rs:42-66` 仅 `inbox.admit` | `run_in_background` 结果滞留，父会话空闲时不消费 |
| K-01 | compaction payload-too-large 免费文本重发是死代码 | `compaction.ts:430-432` → `compaction.rs:525/541`（`sends += 1` 在 send 前，`sends==0` 恒假） | 大附件压缩成功率下降、多耗 1-2 次模型调用 |
| L-01 | 崩溃/重启恢复 sweep 未接线 | `execution/restart.ts:192-231` → 原语在 `store.rs:350-433`/`jobs.rs:561`，无生产调用方 | 重启后孤儿回合、claim、后台通知全部丢失 |
| P-01 | `session.prompt` hook 真实路径从不 dispatch | `prompt.ts:38-51` → `dispatch_session_prompt` 唯一调用点是测试 | 插件对 prompt 的改写/脱敏/路由完全失效 |
| P-02 | prompt 附带 skills 不解析，`skill.text` 恒空 | `prompt.ts:56-76` → `handlers/session.rs:567` 原样存；`to_llm_message.rs:348-353` 只注入 `skill.text` | `@skill` 附带调用静默无效，未知技能不报错 |
| M-01 | per-session model WebSocket transport 整体缺失 | `session/model-transport.ts`（571 行）→ `RequestInput.web_socket` 无消费者，`drain.rs:239 TODO` | 配 WS 的 provider 永远走 HTTP；Move/Reload 不关传输 |
| M-02 | `session.http.request/response`、`ws.*` hook 注册但从不分发 | `model-request.ts:320-375` → `LlmClient::stream(request)` 无 options 参数 | 计费代理/遥测/重写类插件完全无效 |

### 2.2 工具 / shell / 运行时（8 + 1 降级）

| ID | 问题 | ref → rust | 影响 |
|---|---|---|---|
| A1 | `opencode` 命名空间注册时被清空 | ref 保留 namespace → `tool/registry.rs:276` 硬编码清空 | `session_rename/models/session_move` 广告名与 codemode 路径错 |
| A2 | `session_move` 工具恒失败 | ref `Move_` 工具 → `tool/opencode_tools.rs:89-121` 桩 | LLM 无法移动会话 |
| C1 | shell stdout/stderr 顺序读 | ref 并发捕获 → `shell.rs:529-551` 先读 stdout 再读 stderr | 交错丢失；stderr>64KB 互锁到超时 |
| C2 | `shell.create.before` 钩子不派发 | `shell.ts:274` → 生产传 `None` | shell 审计/改写插件失效 |
| C3 | config `shell` 键被忽略 | `config/plugin/shell` → engine 硬编码 `None` | 用户 shell 配置不生效 |
| C4 | `experimental.portable_shell_scanner` 硬编码 false | ref 可切换 → engine 固定 false | 解析器行为与配置不符 |
| C5 | `experimental.subagent_depth` 硬编码 1 | ref 可配置 → engine 固定 1 | 子代理嵌套深度不可调 |
| D1 | `ToolOutcome.output` 在 `ToolRunResult` 边界被丢弃 | `tool/runtime.ts:45-59` → `location.rs:361-366` 只有 content/metadata | **已核验降级为 S2**：v2 的 `session.tool.success`/`ToolStateCompleted` 同样无 output；实际影响限内部 codemode/规范化消费者，需补测试确认 |

> `02` 报告统计写 11 个 S1，明细表列出 8 个；另有 D1 经本报告复核降级。以本表 8 个为准。

### 2.3 AI 协议 / 传输 / 媒体（12）

| ID | 问题 | 影响 |
|---|---|---|
| M1 | WebSocket 传输 + OpenResponses 通道/续链缺失（v2 的 openai-responses **默认**传输） | 网络复用/断线续链/azure WS 头缺失 |
| M2 | 媒体 route 执行层 779 LOC 缺失（auth/轮询/upload-before-submit/cancel/resume） | 22 个已移植媒体协议是死代码 |
| M3 | google-interactions 协议缺失 | thought-signature 无状态回放、previous-interaction-id 续链缺失 |
| M4 | google-speech TTS 缺失 | `speech/mod.rs` 自述只有四种 |
| M5 | xai-responses 适配缺失 | hosted tools `x_search_call`/`image_generation_call`、contextManagement 拒绝缺失 |
| M10 | Bedrock SigV4 无 AWS 默认凭证链 | 仅 Static/Env；profile/SSO/IMDS/EC2 用户无法鉴权 |
| — | Cohere/Mistral 原生协议未接线 | `cohere_chat.rs`/`mistral_chat.rs` 及其 parity 测试运行时不可达；`@ai-sdk/cohere` → UnsupportedProtocol |
| B2 | mantle responses 丢 `store:false` + `include:["reasoning.encrypted_content"]` | Claude-on-mantle 加密 reasoning 回放断裂 |
| — | Azure 完全错线 | 无 resourceName→URL、`/deployments/{model}`、`api-version`、`api-key`；az-cli 凭据不回流 |
| 5.1 | HTTP 超时预算未实现 | 默认 300s 自报错语义缺失；挂起响应头无限等待 |
| — | google-vertex messages/chat 的 URL 与鉴权组合未证实 | messages 走 bearer+catalog URL，ref 为 `x-goog-api-key`/OAuth + `publishers/anthropic` |
| M6/M7 | mantle messages 路由、vercel gateway 路由选项 | 结构化 provider/网关策略不可用（S2 边界，与 S1 同根） |

### 2.4 provider / 鉴权（9，含与 AI 重复项）

| ID | 问题 | 影响 |
|---|---|---|
| A-3 | variant overlay 未并入请求（`#max/#high`） | effort/thinking 配置不生效 |
| A-4 | config 对 catalog provider：apiKey-only 被丢、baseURL 重绑 openai-compatible | 自定义 provider 鉴权/路由错误 |
| A-5 | env 方法过滤缺 vertex/bedrock/cloudflare；bedrock SigV4/vertex project 激活缺 | 这些 provider 的登录方法缺失 |
| A-1 | catalog 刷新后 resolver/integration 注册停在 boot 快照 | 新模型/provider 需重启 |
| D-6 | ChatGPT/Codex 凭据驱动的 provider 整形缺失 | codex baseURL/过滤/成本/头全缺 |
| D-8 | Poe client id 与 token 字段错 | Poe 登录损坏 |
| B-6 | header/chunk timeout 无执行点 | 同 `03:5.1` |
| P-06 | copilot 动态模型同步缺失（KNOWN） | `/models` derive、endpoint 选择、隐藏模型 |
| P-12 | npm provider 包运行时安装加载缺失（KNOWN/设计） | 自定义 npm provider 包不可用 |

### 2.5 HTTP API / client（4）

| ID | 问题 | ref → rust | 影响 |
|---|---|---|---|
| F1 | PTY WS meta 帧三重偏差 | `pty/protocol.ts:14-22` + `handlers/pty.ts:198-199`（replay→binary `0x00+{"cursor"}`，64KiB）→ `handlers/pty.rs:322-329`（meta 先行、文本 JSON、16KiB） | v2 web/desktop 客户端无法识别 meta，断线游标恢复失效 |
| F2 | PTY 正常退出发 4404 | v2 `CloseEvent(1000)`，4404 仅 attach 失败 → `pty.rs:298-305` 正常退出 4404 | 客户端误判会话不存在；原因文案丢失 |
| F3 | persistent-PTY `resized` 缺 `checkpoint` | `handlers/persistent-pty.ts:141-145` → `persistent_pty.rs:333-337` | resize 后参考客户端不能快进，只能全量重放 |
| F4 | WS connect 票据消费缺 Origin 白名单门 | v2 先判 Origin，不通过 403 且不消费 → Rust 无条件消费 | 安全语义偏差（票据可经其他信道泄露时） |

### 2.6 TUI（15，另 2 个 S0）

| # | 问题 | ref → rust | 影响 |
|---|---|---|---|
| 3 | 垂直 tab 布局 + 拖宽 + `tabs.layout` 全缺 | `app.tsx:574-585` → 仅水平 strip；垂直 rail 随旧渲染器退役（`golden_tests.rs:267-269`） | 宽屏布局选择缺失 |
| 4 | TerminalPane 内嵌终端 + `<leader>t/down/up` 缺 | `terminal-pane.tsx` → 键表注册为吞键 no-op（`table.rs:577-581`） | 内嵌终端整面缺失 |
| 5 | `open.menu`(ctrl+o) 错映射 | v2 DialogOpen（会话+项目+worktree 分区）→ 直接开 Sessions ServerList（`command_palette.rs:1805-1817`） | 项目切换半缺失 |
| 6 | 会话列表行详情/项目分组缺 | `dialog-session-list.tsx` → 仅标题行（`command_palette.rs:557-576`） | 长列表可用性差 |
| 7 | 会话过滤 6 项语义错位 | ref trim+case-fold 子串、父排除、空查询全量 → 共享 fuzzy、子会话泄漏、空查询空集 | 搜索结果集不同 |
| 8 | DialogSkill `/skills` 缺 | `prompt/index.tsx:581-583` → 无 dispatcher 入口 | 选中技能无表面 |
| 9 | DialogStash 缺 | `prompt/index.tsx:880-913` → 无 | 草稿 stash 工作流缺失 |
| 10 | DialogImagePreview / diff 图像缺 | `<leader>i` / 附件点击 → 键表-only；diff 图像仅占位文案 | 图像查看交互缺失 |
| 11 | 7 个 slash 缺失：`/share /rename /timeline /fork /compact /undo /redo` | `session/index.tsx:891-1206` → `V2_SLASH` 34 条不含 | 高频斜杠习惯断裂 |
| 12 | composer live `N subagents · M shells` 缺 | `feature-plugins/prompt/footer.tsx:15-45` → 无 | 运行态可见性下降 |
| 13 | cost `$X.XX` 缺 | footer/sidebar → `session.rs:962` `cost: None` | 花费不可见 |
| 14 | shell 模式 `!` 前缀缺 | `prompt/index.tsx:991-1024` → 无 | shell 问答模式缺失 |
| 15 | Zed 编辑器上下文缺 | `editor-zed.ts` → `editor.rs` 仅 `$EDITOR` | Zed 用户上下文注入缺失 |
| 16 | 鼠标选区复制/copy-on-select 缺 | `app.tsx:546-571` → 无 selection→clipboard 路径 | 鼠标复制工作流缺失 |
| 17 | mini 子代理 inspector 缺 | `stream-v2.subagent.ts`/`footer.subagent.tsx` → 无 | mini 下子代理无运行态页脚 |

### 2.7 CLI / config / 打包（6）

| ID | 问题 | 影响 |
|---|---|---|
| N-1 | `--print-logs` 解析后无人读取 | `opencode run --print-logs` / `--standalone --print-logs` 静默无效 |
| N-2 | `run` 早期错误不遵守 `--format json` | 机器消费者收到人类文本（`run/mod.rs:347-355`） |
| N-3 | cli.json `plugins[]` 无消费者；`plugin add` 不按 server/tui entrypoint 分流 | v2 用户的 TUI 插件指令被忽略；TUI-only 插件写错文件 |
| N-4 | server-process 9 个 env 开关无人认领 | `OPENCODE_DISABLE_PROJECT_CONFIG`、`OPENCODE_FILEWATCHER_DISABLE`、`OPENCODE_MODELS_URL/PATH` 等失效 |
| K-1 | `serve --hostname` 仅 IP 字面量，非法值静默回退 127.0.0.1 | `--hostname localhost` 被忽略 |
| K-2 | upgrade 方法集收缩 + 自有更新通道 + 默认端口分叉 | npm/brew 用户升级路径不同（已登记换道） |

### 2.8 插件平台（11）

| ID | 问题 | 影响 |
|---|---|---|
| D-01 | `plugin|plugins` 目录自动发现缺失 | drop-in 本地插件不加载 |
| D-07 | 插件激活从 per-location 变 process-global（KNOWN） | 多 location 下插件只看到 boot 目录 |
| D-10 | `ctx.session` 15→11 暴露、server 端口仅 5 方法（KNOWN） | create/switchAgent/command/compact/move 不可用 |
| D-11 | `ctx.permission` 仅 reply 且空响应 | list/get/hook 缺失，权限自动应答不可用 |
| D-13 | serve 模式插件 `event.subscribe` 无事件源 | headless 下事件驱动插件完全失效 |
| D-14 | 插件 RPC 全链 STUB | `/api/rpc` 恒 500，`ctx.rpc` 不可用 |
| D-18 | 插件 KV 落 `/tmp/opencode` sandbox | 重启即失，插件状态/缓存/onboarding 丢失 |
| D-21 | guest 插件 hooks 不进引擎分发链 | **JS 插件全部 hook 能力生产不生效**（`host_slot:()`，`hooks.rs:1423/1506`） |
| D-30 | 远程 MCP OAuth 登录流缺失（KNOWN） | OAuth 类 MCP 到 401 即 `needs_auth` 死端 |
| D-33 | 插件注册的 MCP 不进真实注册表 | 同 S0-5 |
| D-35 | 插件注册的 skills 不进引擎技能表 | 插件捆绑技能不可见 |

### 2.9 数据库 / project / worktree（6）

| ID | 问题 | 影响 |
|---|---|---|
| D1 | project id 用 SHA-256，v2 用 SHA-1 | 共享/导入 v2 库时同一 repo 产生双 project 行，旧会话无人认领 |
| D2 | `Project.activate` 缺失（60s 限频 time_active 提升） | 项目列表「最近使用」排序失效 |
| D3 | 生产解析器挂 `NoEvents` | `worktree.resolved` 无生产者、adopted 扫描为死代码 |
| D4 | `Project.update` 忽略 canonical/commands、伪造响应、不发事件 | 项目更新无效/客户端状态错 |
| D5 | `worktree.remove` 只删 DB 行 | 不跑 `git worktree remove`、无脏树/force 语义 |
| D6 | `worktree.refresh` 空桩（KNOWN） | git worktree 变更不重扫 |
| D12 | v1 内容迁移（session/message/part，ref 1135 行）未移植 | 状态路由恒答 completed，旧会话内容未迁移 |

### 2.10 core 杂项 / VCS（4）

| # | 问题 | 影响 |
|---|---|---|
| 1 | `/api/vcs/base` 缺必填 `source`；base 语义用 `merge-base @{u}` 而非 reflog/default | 强类型客户端解码失败；无可用 base 应 503 |
| 2 | `/api/vcs/branch` 返回 `{"branches":[...]}` 而 schema 是 `string[]` | 客户端解码失败；仅本地分支、无排序 |
| 3 | `/api/vcs/diff` 返回单条伪 `FileDiff`，忽略 branch/committed、无 10MB 封顶 | Rust TUI 自身 diff/branch 视图不可用 |
| 4 | `worktree.remove` 只删 DB 行 | 同 `09:D5` |

---

## 3. S2 登记（按子系统压缩；完整证据见各明细报告）

### 3.1 会话 / runner（27）

- **生命周期**：`L-02` 插件桥 `engine.prompt` 绕过 prepare 且阻塞；`L-03` 新输入不提交 staged revert；`L-05` move 目的地校验缺失（KNOWN）；`L-06` shell 通知 delivery 用 Queue 而非 steer；`P-03` 图片 normalize 缺失（KNOWN）；`P-04` `plugins.awaitActivation` 门槛缺失。
- **上下文/技能**：`C-01` reference guidance 恒「无引用」（KNOWN）；`C-02` skill changed 不走增量 diff；`C-03` 远程 URL 技能源不下载；`C-04` select 前不 flush MCP 工具。
- **模型请求**：`M-03` route hook 后 ProviderContext 兼容检查缺失；`M-04` promptCacheKey 大写 hex 误判；`M-05` 无工具时错误文案不同。
- **step/publisher**：`T-01` progress 不进 record（中断 subagent 错误缺 sessionID 后缀）；`T-02` fragment state append/end 整体替换而非合并；`T-03` delta 批处理无 100ms 定时器；`T-05` steer 型 compaction 重置 step；`E-01` Step.Started 与 pending delta 顺序（理论性）。
- **compaction**：`K-02` 手动 native 重复发 `Started(reason=auto)`；`K-03` hook 短路结果缺失（KNOWN）；`K-04` 取消不发 interrupted + spent 泄漏；`K-05` `/compact` 默认 Queue 而非 steer；`K-06` native 不传 `webSocket="session"`。
- **有意 EXTRA**：`R-01` title 并发重试 ≤20（正向）；`R-02` `Superseded` 中断原因（wire 枚举超集）；`R-03` 失败回合合成 step/error part（正向）；`R-04` stopping 竞态恒 interrupted；`R-05` title `""` 再生缺失（KNOWN）；`R-06` usage fan-out 屏障（正向）；`R-07` Tool.Error 嵌套 cause 解包缺失。
- `S-02` completion 文本过滤更严格（正向/边缘）。

### 3.2 工具 / 运行时（22）

- **工具行为**：`A3` worktree 指引 system part 未接线；`A4` `models` own-provider 排序失效；`A5` ripgrep 自动下载缺失；`A6` Image/photon 归一化缺失（KNOWN）；`A7` `list_mcp_resources` 内容固定文本；`A8` question 多选题退化单选；`A9` websearch provider 被 env 键门控。
- **文件工具**：`B1` edit 输出缺 `files`；`B2` read 二进制检查范围过宽；`B3` 建议列表排序不同；`B4` AGENTS.md 发现未走 realpath；`B5` read 整文件读入（无 256KB 分块）；`B7` snapshot enabled 门缺 vcs 判定；`B8` formatter no-op（KNOWN，默认等价）。
- **shell/PTY**：`C6` shell `progress({shellID})` 未发布；`C7` shell metadata 不全；`C8` 超时击杀无 TERM 宽限；`C9` persistent-PTY daemon 边界（KNOWN）；`C10` Pty 默认命令解析器未接线；`C11` 每小时保留清扫未启动；`C12` rg 退出码/错误映射偏差；`C13` session env 未按 workspace 门控。
- **输出/搜索**：`D1` 结构化 output 丢弃（本报告复核降级，见 §5）；`D2` config `tool_output` 无效；`D4` `/api/fs/find` 朴素子串替代 fuzzysort 索引。
- **权限 metadata**：`E1` patch filepath 顺序/去重；`E2` webfetch metadata 不完整；`E3` websearch 表单缺 `kind`；`E4` patch 自动建父目录（ref 失败）；`E8` webfetch 超时/错误文案。

### 3.3 AI（14）

- 缺失/未接线：`M6` mantle messages 路由；`M7` gateway-protocol；`M8` generateObject（KNOWN）；`M9` experimental（KNOWN）。
- 路由/默认：meta messages adaptive thinking 默认缺失；vercel gateway 路由选项不序列化；mantle effort 门控缺失；`B4` bedrock/mantle 测试覆盖不足。
- 其他：digitalocean fixture 未 pin；`abort` 无专门 parity 测试；`sanitize` no-op（KNOWN）；`aisdk.sdk/language` host 内部对象（永久登记）。
- 协议覆盖 217/226 provider；9 个 aisdk shim 返回干净 `UnsupportedProtocol`。

### 3.4 provider / 鉴权（10）

- `A-6` `${VAR}` baseURL 替换与 `UnresolvedProviderVariablesError` 缺失；`A-7` `model.default` 不校验 enabled/available；`A-8` 本地 provider 动态发现缺失；`A-9` bedrock profile-only 模型/别名过滤缺失。
- `D-4` chatgpt id_token RS256/JWKS 验签缺失（KNOWN）；`D-5` chatgpt hostID/savedClientID/旧方法迁移缺失；`D-7` xai browser→device 迁移、错误文案、JWT exp 回退、WS 默认缺失。
- `G-1/2` CLI 交互集成选择器/URL 自动打开/表单提示；`G-3` logout env 提示 action 文案错（"use" 永不触发）；`P-17` OAuth 回调页简化；`C-2b` 非默认 models.dev 源 hash 后缀键缺失；`D-9` copilot entitlement 文案/分支；`D-10` azure key 表单恒显示。

### 3.5 HTTP API（18）

- **KNOWN 桩**：`F5` plugin.check 空清单；`F6` plugin.update 恒 204；`F7` `/api/rpc` 恒 500；`F8` worktree.refresh no-op；`F9` stats 无分桶；`F10` vcs.diff 单合成块；`F11` title `""` 不再生成；`F12` SSE 多 3 类事件（`mcp.prompts.changed`/`mcp.tools.changed`/`session.diff`）。
- **NEW**：`F13` 过期配对链接 401 多 `www-authenticate`；`F14` pre-ready info 多 capabilities；`F15` 通配绑定 urls 不展开；`F16` `attached.role` 取请求值；`F17` framed-input 校验过松；`F18` 无效 UTF-8 lossy 而非丢帧；`F19` resize 丢 attachmentID；`F20` activate 在 replay 前（帧序竞态）；`F21` client `Last-Event-ID` 扩展（良性）；`F22` routes.gen 的 `projectID` required 标注错。
- OpenAPI 141/141 逐字节一致；SSE 公共事件缺失 0、多余 3；SDK 线上契约 1:1，29 个纯命名差异。

### 3.6 TUI（17）

- `#18` OSC 0/2 标题缺失（TM-5）；`#19` sync-2026 缺失（TM-6）；`#20` DevToolsBar；`#21` MigrationOverlay；`#22` plugin 路由/`OPENCODE_ROUTE`/stats/plugins/storybook；`#23` Settings 仅 4 行子集；`#24` 通用 ErrorDetails 缺；`#25` 通用 alert/confirm（含 `ctx.ui.alert/confirm`）；`#26` masked-text；`#27` diff 右键菜单/导出；`#28` 目录 recents/破坏性删除（UNVERIFIED）；`#29` `app.console` 控制台；`#30` SessionLocationMissing 屏（UNVERIFIED）；`#31` `toast.activate` EXTRA；`#32` 插件事件空 payload（服务端 seam）；`#33` verbosity cycle 吞键；`#34` palette 缺 7 族命令。
- 已确认修复：auditB 的 12 个 STUB 清零、21 个 MISSING 已修、键位 231/231、主题 30 资产语义零差、TM-1/2/3/4 已修。

### 3.7 CLI / config（14）

- `F-3` session export 无交互选择器；`F-4` import 不校验 schema/不 resolve directory；`F-6` pair 无 QR；`F-9` api 空格编码 `+` vs `%20`、header 无 100 上限；`F-10` UpdatePreflight 缺失；`F-11` OTEL 缺席；`F-12` uninstall 不展示包管理器 removal/PATH；`F-13` stats 时区/超时文案；`N-5` `debug paths db` 绕过 `OPENCODE_DB`/channel；`N-6` OTEL（重复）；`K-3` stats 日历/范围校验；`K-4` session list 分页器/本地时间；`K-5` 枚举/范围校验族；`K-6` 全局 flag 作用域超集；`K-7` pair QR/API 编码；`K-9` 日志布局 `server.log`/`tui.log` vs `opencode[-ch].log`。
- 已修复确认：根 `--prompt`/`--auto` 转发、mini replay 回退、cli.json 写回+迁移、v2→rust state 根迁移、provider 超集误报更正。

### 3.8 插件平台（9）

- `D-02` 热重载无生产调用方；`D-03` plugin check/update 空转（KNOWN）；`D-04` guarded plugin id remove 保护缺失；`D-05` v1 插件仅容忍忽略；`D-15` `shell.create.before` 无生产触发；`D-16` tool editor 缺 `namespace()`；`D-22` `session.generate` hook 并入 `session.context`；`D-23` http/ws hook 无 dispatch（KNOWN）；`D-24` `permission.evaluate` 生产为 None（KNOWN）；`D-31` MCP protocol pin/旧 SSE-only 传输；`D-34` 凭据切换不重连 MCP；`D-40` 内置集成 provider 缺 4 项（modal/cloudflare×2/bedrock）。

### 3.9 数据库 / project（14）

- `D7` worktree.list 排序/当前 location 过滤；`D8` project.list 缺 `asc(id)` 平局序；`D9` 新项目 time_active 默认 0（ref now）；`D10` `KV.scan` 缺；`D11` Repository/RepositoryCache 整体缺失；`D13` Workspace 服务缺失（主线不影响）；`D14` announce `project.updated` 载荷不全/非 canonical 位置发 `{}`；`D15` 事务默认 BEGIN IMMEDIATE（ref deferred）；`D16` State 内核无通用对应；`D17` Instance `discovery:false`/出生插件表缺失；`D18` LocationRef 全平台反斜杠归一；`D19` TTL sweeper touch 未接线；`D20` canonical 主 worktree 用 commondir 启发式；`D21` remote URL 规范化只认 4 scheme；`D22` worktree 行/事件非同事务、无 announcing 去重；`D24` channel 默认 latest vs 构建期（KNOWN）。
- 静态层 19 表/48 迁移/pragma/锁/事务/路径规范化全 MATCH，并有 4 项加固。

### 3.10 core 杂项（22）

- `#5` 图片 normalize/resize 全缺；`#6` reference：git 源不物化、`/api/reference` git path 填仓库 URL、guidance 桩；`#7` shell/tool-output 保留清理未启动；`#8` snapshot symlink/context/10MB/二进制/gitconfig-seed 分歧；`#9` vcs info/status 细节（branch.default、detached HEAD、additions/deletions 恒 0、scope、排序、init 错误、hg provider）；`#10` modal 动态模型；`#11` websearch 凭据/retry-after/preferred 清理/错误类型；`#12` codemode `pinned`；`#13` wellknown login-command 注册面；`#14` workspace 生命周期（主线不影响）；`#15` environment 执行平面缝（架构性）。
- 其他映射表中的 PARTIAL：codemode catalog 预算、git sync/clone、vcs patch 上限、v1 迁移已通过。

### 3.11 包级缺失表面（21）

- **P0 主程序功能**：`plugin-browser` 的 agent `browser` 工具；TUI 内建 LaTeX + Mermaid 渲染（`builtins.ts:24-26` 默认注册）。
- **P1 用户可见生态**：npm `@opencode-ai/sdk`（JS SDK 形态）、VS Code 扩展、GitHub Action、npm/AUR/brew 安装渠道、macOS/Windows/aarch64 二进制（release.yml 仅 linux-x64）。
- **P2 开发工具**：containers、simulation、storybook、http-recorder 录制端、httpapi-codegen client 生成、script、effect-drizzle-sqlite、nix flake、identity 资产。
- **P3 托管服务/营销**：console/*（6 子包）、enterprise、function、stats/*、posts、services/{www,update,files}。其中 console 客户端集成与升级换道已有 Rust 对应。
- **书面 out-of-scope**：desktop、web、gui-extensions（`ROADMAP.md:7-13`）。

---

## 4. 跨 lane 冲突 / 纠错（本次审计的关键增量）

| 冲突 | 结论 |
|---|---|
| 工具 `D1`：结构化 output 全链路丢失 | **降级为 S2（内部）**。复核 ref `session-event.ts:531-539` 与 `session-message.ts:139-144`：v2 的 `session.tool.success` 和 `ToolStateCompleted` **同样没有 `output` 字段**。影响仅在 `NormalizedResult.output` 的内部消费者（codemode/规范化），不是 wire 协议缺口；需补 codemode 取值测试。 |
| websearch 是否解析 Retry-After | **两者都对但层面不同**：429 冷却轮换已移植（`websearch.rs:210-247`），但冷却时长固定 60s，未解析 `Retry-After` 秒/HTTP 日期（ref `websearch.ts:215-221`）。归入 `11:#11` S2。 |
| auditE 说 `session.shell` 把命令发给 LLM | 已修复（当前实现走 shell 服务）。fork 边界 bug、generate 503、session.diff `[]`、worktree.create 不跑 git 也已修复；这些旧账不应再计入。 |
| auditE 说 integration/auth host 全缺 | 登录流程本体已完整（10 个 OAuth/命令流、credential、wellknown）；缺的是「凭据→请求」最后一公里（04:B-1/2/3），以及 4 个集成 provider。 |
| ROADMAP 称插件 guest hooks "wired in the engine lane" | **与代码不符**：`PluginSupervisor.host_slot: ()`、`Bridge::dispatch_hook` 无生产调用方。FINDINGS.md 的「engine-side adapter does not exist」才是事实。 |
| auditD 说 SSE 缺 20 类事件 | BE-1 后已清零；当前 0 缺失、3 多余（`mcp.tools.changed`/`mcp.prompts.changed`/`session.diff`）。auditD 的 ref 89 与本次 95 是清单展开口径差异。 |
| auditD 说 `mcp.tools.changed` 是 parity | 细化纠正：ref 公共 manifest 只含 `mcp.status/resources`；`tools.changed` 为 Rust 多余。 |
| 02 报告写 11 个 S1 但明细表 8 条 | 以明细与本总表为准：8 条；D1 降级。 |
| `README.md` 说 Web UI 不在范围 | **与 ROADMAP R-04 冲突**：Web UI 已按 R-04 构建参考 app 归档并嵌入（`release.yml` web-assets + `app_archive.rs`）。应改 README。 |
| `Tool D1` 说 publisher 无 output 是缺陷 | ref publisher 也无；同上，降级。 |
| `07` 更正 auditF：provider `timeout/headerTimeout/chunkTimeout/transport/compaction` | 不是 Rust 超集，ref `provider.ts:11-18` 本就有这五项，实为 1:1。 |

---

## 5. 修复优先级（建议）

### P0 — 先恢复主流程（约 8 个改动簇）

1. **凭据通用化**（`04:B-1/B-2/B-3`）：resolver 注入 active credential；console/zen public-key 降级；`/api/model` 可用性读凭据。一次改动覆盖 3 个 S0。
2. **插件 guest 接线**（`08:D-21/D-18/D-33/D-35` + `06:#1`）：把 `Bridge::dispatch_hook` 注册进 `HookRegistry`、KV 落 DB、MCP/skills 快照进引擎、补 `ctx.ui.slot/router/keymap.layer`。
3. **后台任务唤醒 + 崩溃恢复**（`01:S-01/L-01`）：completion 后 `execution.wake(parent)`；boot 时调用 restart sweep。
4. **VCS/HTTP 线格式**（`11:#1-4`、`09:D5`、`05:F1/F2`）：三端点按 schema 重写；`worktree.remove` 跑 git；PTY meta 改「replay→binary 0x00+JSON，64KiB」，正常退出 1000。
5. **超时/死锁**（`03:5.1`、`02:C1`）：reqwest header/chunk/total timeout；shell 双流并发汇入。
6. **插件 MCP OAuth + RPC**（`08:D-30/D-14`）：否则 `/connect`、远程 MCP、`/api/rpc` 全是死端。
7. **主 TUI 表单阻塞面**（`14:TD-16/TD-17`）：补 `FooterView::Form` 的 frame/press 分支，字段类型补 `boolean/number/integer`，表单期间禁用 composer。
8. **HTTP 线格式 + 请求校验**（`12:H0-H5`）：cursor null-vs-absent、cursor/order 规则、`uriTemplate`、stats 形状、`WorktreeError.name`；由 OpenAPI 生成统一请求校验中间件覆盖 ~139 条路由。

### P1 — 用户日常可见

7. TUI：7 个 slash、垂直 tab、TerminalPane、选区复制、cost/live 计数、shell `!`、Zed、DialogSkill/Stash/ImagePreview、mini subagent inspector（`06:#3-#17`）。
8. provider/AI：variant overlay、Azure、Cohere/Mistral 接线、WS transport、Bedrock 凭证链（`04:A-3/A-4/A-5/D-6/D-8`, `03:M1-M5/M10/B2`）。
9. CLI：`--print-logs`、run JSON 错误、cli.json plugins、9 个 env 开关、hostname（`07:N-1..N-4/K-1`）。
10. 缺失表面：browser 工具、LaTeX/Mermaid、多平台二进制（`10:P0/P1`）。
11. **cli.json 读侧 + 会话保留**（`13:T-1/T-3/T-4`, `13:C-8`）：`session.permissions`、`session.*`、mini 8 键、`evict(root)`；这是长跑内存与用户配置生效的直接面。
12. **managed-policy / compaction 配置**（`13:C-7/C-1`）：企业策略、`compaction.auto/keep/buffer`；若目标用户含组织托管则 C-7 提到 P0。

### P2 — 一致性与外围

11. 数据库生命周期：SHA-1 project id、activate、NoEvents、Project.update、v1 内容迁移（`09:D1-D4/D12`）。
12. 工具 metadata/截断/清理任务、snapshot symlink/context、reference 物化、image normalize（`02`, `11`）。
13. CLI/日志/schema 校验族、OTEL、升级通道（`07` S2）。
14. 插件热重载、guarded id、MCP protocol pin、集成 provider 缺 4 项（`08` S2）。
15. TUI 低频面：OSC 标题、sync-2026、DevTools、MigrationOverlay、plugin 路由、通用 alert/confirm、masked-text（`06` S2）。

### P3 — 生态/托管

16. npm SDK、VS Code、GitHub Action、npm/AUR/brew、console/enterprise/stats/posts/www 等。desktop/web/gui-extensions 维持 out-of-scope。

---

## 6. 覆盖与未验证声明

### 已覆盖
- 参考侧：`packages/{core,ai,server,protocol,sdk,cli,tui,schema,plugin,theme,util,app}` 的路由/组件/命令/协议/迁移清单；根入口、install、release workflow、`package.json`。
- Rust 侧：全部 12 个 crate + `tools/openapi-gen`，约 345k LOC 的文件级映射与关键调用点 grep。
- 第二轮补扫：141/141 HTTP handler 字段级、config/env/policy、TUI 会话路由行为级、事件 payload 字段级、Copilot/媒体协议。
- 既有登记项：ROADMAP、FINDINGS、MIGRATION、PORT_REPORT、docs/auditA-F、rescan/reposcan/scan-ab/termatrix/final-sweep。

### 未覆盖 / 需运行时复核
- 未跑 `cargo build/test`，未跑 TUI 双端 live 差分；所有「无调用方」结论基于当前树 grep，可能被 feature gate/动态注册绕过（已尽量用 `grep -v tests` 与生产入口核对）。
- `04:B-3`（keyless zen）为静态推断，需真实请求复现。
- TUI 会话路由已在第二轮做行为级深扫，但未做逐行 diff；主题 token 消费面、frecency 细节、部分交互仍标 `UNVERIFIED`。
- AI 协议以 cassette/expected-dump 测试为准，未覆盖所有 provider 的真实网络行为。
- 真实插件加载、真实 MCP OAuth、真实 v2 DB 导入、Windows/macOS 平台行为均未做端到端验证。
- 参考基线为 v2.0.24（commit `e7a34f09bf`）；用户实际 v2.0.25 的差异不在本次范围。

---

## 7. 第二轮补扫增量（2026-10-18）

第一轮之后又跑了 5 条补漏通道，新增 **108 条**记录（1 S0、约 25 S1、约 82 S2/S3）。它们不是重复项，而是第一轮明确未深扫、或方法上覆盖不到的层。以下只列新增结论；完整证据见 `12`–`16`。

### 7.1 新增 S0：主 TUI 表单阻塞面缺失

见 §1 `S0-6`。一句话：`form.created` 进入 footer 队列后没有任何主 TUI 渲染/按键分支，表单会假死；`FieldKind` 缺 `boolean/number/integer` 时整张表单还会在反序列化阶段被静默丢弃（`14:TD-16/TD-17`）。

### 7.2 新增 S1（25 项）

| ID | 问题 | 证据 |
|---|---|---|
| H0 | **请求校验中间件整体缺失**：v2 对 141 条路由的 payload/query 做 Effect schema 校验；Rust 只校验 `session.create` 与 `session.list` 两处，其余 ~139 条错型/缺必填静默降级 | `12:H0`, `schema_check.rs:6-11` |
| H1 | `message.list` 的 cursor/order 规则反转：v2「同给→400、仅 cursor→用 cursor 内 order」，Rust 同给时按 order 校验，正常翻页第 2 页 400 | `12:H1` |
| H2 | 分页 cursor 输出 `{"previous":null,"next":null}`；v2 optional 在 absent 时省略键，强类型 SDK 的 `optional` 不接显式 null | `12:H2`, `session.rs:324`, `message.rs:119` |
| H3 | MCP resource catalog 模板输出 `uri`，schema 必填 `uriTemplate` | `12:H3`, `mcp.rs:137` |
| H4 | `session.stats` 响应形状破坏：`sessions` 是对象而非数字，缺 range/tools/activity 等 8 个必填字段 | `12:H4`, `handlers/session.rs:1789` |
| H5 | `WorktreeError.name` 填机器码（`invalid_name` 等），schema 要求字面量 `"WorktreeError"` | `12:H5`, `project.rs:313` |
| TD-01 | 六类 wire 消息被 TUI 双摄取路径静默丢弃：`shell / agent-switched / model-switched / location-switched / system / synthetic` | `14:TD-01` |
| TD-02 | 消息导航命令族 5 条注册即死；MessageActions「Jump to」不跳转 | `14:TD-02` |
| TD-03 | `session.parent`（子会话内 `up` 返回父会话）无实现 | `14:TD-03` |
| TD-04 | 重连恢复链缺失：无 `session.sync(children)/permission/form` resync，断线期间权限/表单永久丢失；「Session not found」收尾缺失 | `14:TD-04` |
| TD-05 | 挂载预算与历史分页缺失：全量加载、无 tail/backfill 窗口 | `14:TD-05` |
| TD-07 | 队列管理器缺 undo；steer/delete 为本地重发语义；pending steer 无区分表面 | `14:TD-07` |
| TD-17 | `FieldKind` 无 `boolean/number/integer`，`form.created` 反序列化失败后整表单被丢弃 | `14:TD-17` |
| TD-18 | 权限交互体用 mini footer 三段机而非会话路由两段机：always 多一次确认、esc 语义反转、自由文本 reject 对所有会话开放 | `14:TD-18` |
| TD-19 | edit 权限的 diff 预览体缺失 | `14:TD-19` |
| C-7 | **managed-policy 运行时整体缺失**：`experimental.policies`、v1 迁移产物、Console 下发语句三源死码；`provider.use` 目录裁剪与 `permission` 策略拒绝两执行面全缺；`provider-use` 字面量也错 | `13:C-7`, `managed-policy.ts:9-32` |
| C-8 | **TUI session-retention 缺失**：`by_session` 只增不减，`session.deleted` 只关 tab 不清缓存；长跑内存无界 | `13:C-8`, `store.rs:501-503` |
| C-1 | `compaction.{auto,keep.tokens,buffer}` 零接线：`Settings` 结构存在但无构造点，`auto:false` 被忽略 | `13:C-1`, `compaction.rs:40-51` |
| T-1 | cli.json `session.permissions`（prompt/autoaccept）读侧缺失，仅 `--auto` flag 生效 | `13:T-1` |
| T-3 | `session.{thinking,verbosity,tps,markdown,scrollbar,grouping}` 读侧死键簇；grouping「只写不读」 | `13:T-3` |
| T-4 | mini 显示 8 键全 dead（thinking/tools/shell_output/turn_summary/footer/splash/work_spinner/mono） | `13:T-4` |
| EV-1 | `persistent-pty.added` 的 `terminal` 整体错形：snake_case、`size/output` 序列化为二元数组而非 `{cols,rows}`/`{head,tail}` | `15:EV-1`, `persistent_pty.rs:114-129,677-684` |
| C1(Copilot) | Copilot 只覆盖 chat lane：`gpt-5+ / grok- / gemini- / mai-code-` 应走 `/responses` 的 21/34 模型打错端点 | `16:C1` |
| C2(Copilot) | `reasoning_opaque` 多轮推理续链全缺 | `16:C2` |
| Vertex | google-vertex messages 无 base 时静默落 `api.anthropic.com` | `16:任务2`, `03:V2` |

### 7.3 新增 S2/S3（按域压缩）

- **HTTP 字段（12）**：cursor 内容丢 search/order；`credential.create` 的 `activate` 默认值反转；`permission.saved.list` 缺省返回全项目；`session.active` 位置域 vs 进程域；`session.context` 缺 compaction 过滤；export sanitize 只删 metadata 而非密文替换；instruction 上限 32K vs 256K；`shell.remove` 幂等性反转；另有 13 条 S3 信封字段/文案细节。
- **config/env/policy（13）**：`warming` dead、`watcher.ignore` dead 且 `filesystem.changed` 通道缺失；`diffs.{source,wrap,tree,single,view}`、`scroll.{speed,acceleration}`、`prompt.{paste,image_preview}`、`debug.{timing,turn_tokens}`、`animations`、`experimental`、`tabs.indicators`、`session.new_location` 全部解析后丢弃；`OPENCODE_LOG_LEVEL`、`OPENCODE_CLIENT` 服务端读者、`OPENCODE_FAST_BOOT`、`OPENCODE_SHOW_TTFD`、`OPENCODE_DIRECT_TRACE`、`OPENCODE_NODE_ASSETS_DIR` 缺；`OPENCODE_DISABLE_MODELS_FETCH` 仅测试假认领。`TuiConfig::from_info` 只消费约 14/70 个 leaf。
- **TUI 深扫（14）**：AssistantRetry 倒计时、BackgroundToolHint、TurnTokenUsage、`session.toggle.thinking/scrollbar` 吞键、`/rename <title>` 带参、`/copy`、QueuedPromptDock/Jump-to-latest 不可点击、`session.new_location:"inherit"` 不生效、无客户端会话数据逐出、Experiments 对话框缺、权限展示行丢工具输入、表单深度子集（校验/条件/单字段直答/tab条/review/草稿）。
- **事件 payload（15）**：`pty.created/updated` 的 `info.exit_code` 同病；`filesystem.changed` 恒 `"change"`（删除应 `"unlink"`）且缺 realpath；`session.usage.recorded` 泄漏到公共 SSE（公共流实际多余 4+1）；Rust TUI 缺 `session.execution.*` → idle 合成；`credential.switched` 的 null→absent round-trip 不对称。
- **Copilot/媒体（16）**：`X-Interaction-Type/Id` 从不发送（函数死代码），title/compaction 误发 `x-initiator:user`；`/v1/messages` Claude-on-Copilot lane 缺；utility title 缺；copilot providerOptions/User-Agent 缺；AssemblyAI 误拒 `prompt/speakers`；22 个媒体协议仍全部是死代码；9 个不可绑定 provider 影响 434/8390 模型（5.2%）；11 个 provider 的 `prompt_cache_key` 永不发送。

### 7.4 第二轮带来的结论变化

1. **S0 从 5 增至 6**：表单阻塞面是新增的主流程断裂；它也与字段类型缺失叠加。
2. **“HTTP 路由完全对齐”需要修正**：路由表/OpenAPI 一致，但 handler 实际 payload 和请求校验仍有系统性差异（H0–H5）。
3. **“事件类型集合已清零”不足以说明事件对齐**：payload 字段仍有 S1/S2（EV-1 等）。
4. **“config schema 全对齐”只说明类型对齐**：约 70 个 cli.json leaf 只有 14 个被消费；managed-policy / session-retention / new-session-location 是整子系统缺失。
5. **Copilot 从“部分可用”下调**：chat 子集存在，但 responses lane 缺失导致约 62% Copilot 模型端点错误。
6. **纠错**：`command.executed` 是 v1 legacy-only，v2.0.24 双侧均不发布（`15:EV-10`）；`04:B-5` 的 x-initiator/Vision 头已过时；`05:F12` 的“多余 3 类”应细化为 4+1。

### 7.5 仍然没有被证明覆盖的面

- **运行时/并发**：inbox wake、compaction interrupt、PTY attach/resize、断线重连、kill -9 恢复，只有静态接线判断。
- **真实网络**：provider 错误体、限流、代理、超时、流式中断；cassette 不等于真实网络。
- **真实插件**：omo-slim 等未实际加载；TUI slot/KV/RPC 的端到端行为未验证。
- **真实 v2 DB 接管**：SHA-1/内容迁移/迁移版本只做了静态对比，未做真实库导入回放。
- **平台**：Windows ConPTY/路径/信号/service、macOS、aarch64；当前 release 仍只有 linux-x64。
- **性能/资源**：会话保留缺失、清理任务未启动、长会话内存/磁盘增长。
- **Web UI**：仅按“参考 app 构建产物 + 服务器契约”间接覆盖，未做浏览器端到端。

---

## 8. 明细报告索引

| 文件 | 内容 |
|---|---|
| `01-session.md` | 会话生命周期、prompt/context、runner、compaction、retry/title、subagent |
| `02-tools.md` | 内置工具、file-access/mutation、snapshot、shell/PTY、tool-output |
| `03-ai.md` | 协议/provider/route/媒体/超时/缓存/错误 |
| `04-provider-auth.md` | provider 目录、model resolver、credential、OAuth/登录流 |
| `05-api.md` | 141 路由、SSE、PTY WS、client/SDK、OpenAPI |
| `06-tui.md` | 路由/对话框/组件/键位/主题/动画/插件 UI/mini |
| `07-cli-config.md` | CLI 命令/flag、config/cli.json、schema/util、安装/迁移 |
| `08-plugin-platform.md` | 插件宿主/API/hooks/KV/RPC、MCP、集成、权限、表单 |
| `09-database.md` | 19 表/48 迁移、project/worktree/location、KV/state |
| `10-surfaces.md` | 包级映射、缺失包、入口/产物/平台 |
| `11-core-misc.md` | VCS/reference/image/modal/git/workspace/wellknown/websearch |
| `12-http-payloads.md` | 141 endpoint 的 handler 实际请求/响应字段矩阵、请求校验、错误信封 |
| `13-config-env-policy.md` | 全部 config leaf、OPENCODE_* env、managed-policy、session-retention、new-session-location、平台 |
| `14-tui-deep.md` | 会话路由行为、Permission/Form 深度、Experiments、retention/new-location 的 TUI 表现 |
| `15-event-payloads.md` | 事件 payload 字段级生产者/消费者矩阵、公共流过滤 |
| `16-copilot-media.md` | Copilot chat/responses 全能力、媒体协议、未引用 AI 文件、prompt-cache-key |

---

*本报告只读审计产物，未修改任何产品代码。*
