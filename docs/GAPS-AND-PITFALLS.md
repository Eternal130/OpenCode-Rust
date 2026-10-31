# 移植缺口与坑（Gaps & Pitfalls）

> **2026-10-31 第六轮复核**：对文档现存未关闭条目逐项现状验证——G-4/G-6/G-8 已悄然修复收口；G-1 残留缩小（mcp/stdio 已迁）、G-24/G-31 部分清偿；G-34/G-41..G-48 确认仍在。详见文末第六轮。

> **2026-10-31 第五轮全量对比审查**：9 通道全量对读 + 4 通道缺口复核，新立 G-50..G-78（含 5 项🔴：G-50 snapshot 保真、G-51 persistent-pty Origin、G-52 debug config 脱敏、G-53 skill 输出、G-54 run 表单退出码），订正 G-17 断言与勘误 #4 半边，撤销 12 项初报。详见文末第五轮。

> **2026-10-31 收口轮（gap-burndown-3）**：G-18..G-27、G-29..G-31、G-33..G-37 全部落地并合并 main（单 lane 串行、worktree 隔离、dev-gate、合并门测试，日志在 .test-logs/merge-*）。残留清单与 G-1/G-6/G-8 残留项同轮清偿（MCP stdio 经 Environment seam、Shell 查看 1s 轮询、https/socks5 WS 代理）。G-26 双栈信任统一（oc-http crate，platform-verifier + webpki 降级）。G-28 维持有界化设计（声明为插件生态 breaking change）、G-32 v1/legacy schema 维持有意不移植（纯 v2 面无影响）——两项裁决为文档化决策。仍开放的工程残留：G-1 tools/handlers 全量 consumer 迁移（read/write/edit/shell/glob/grep/patch、pty）、G-24 有界外项（guest 侧 worktree host-op 调用、vcs.transform/reload、websearch editor.default.set 持久化、worktree 注册表热重载）、G-31 内存索引（10s 刷新）。
>
> 2026-10-30 二次复核版；同日 burndown 完成状态更新（gb2-burndown，commits a193f4b..phase-6）。每条均经两侧源码取证（rust `/opt/opencode-rust` vs ref `/opt/opencode-ref`），未采信仓库内既有评审文档。总报告见 `COMPREHENSIVE_PORT_AUDIT.md`。
>
> 状态图例：✅ 已解决（本轮 burndown 落地）· ❌ 缺失 · ⚠️ 部分/偏差 · ℹ️ 有意决策（已文档化）· 🧹 清理项
>
> **2026-10-30 第三轮全量对比审查**：10 条并行通道对 ref 全部功能逐模块对读后，又对所报缺口做了 3 通道代码级二次核验。确认缺口收录于下方 G-18..G-40；8 条原报缺口经核验撤销（见文末"勘误"）。总报告见 `PORT_COMPARISON_FINAL.md`。

## 高优先级

### G-1 ✅ workspace 沙箱执行层（environment/）已移植
- 2026-10-30 落地：`crates/oc-core/src/environment/`（files/driver/exec_defaults/local/memory/unavailable/workspace/mod + tests，9 文件）——Files trait（lstat-vs-follow 语义、64MiB 读上限、move into-dir join）、exec-defaults sh 协议（退出码 44/45/46、LC_ALL=C、GNU stat/find/dd，字节级对齐 ref）、WorkspaceService（create/provision/connect/destroy + WorkspaceDriver provider registry，local 默认）。
- Environment 服务按 location 绑定（workspace_id → connect，否则 local driver）；`oc-server/state.rs` AppState 已接线。
- 已知有意残留（workspace.rs 模块文档列明）：consumer 迁移（tools/handlers 改走 Files）为后续工作；ref 的共享 provision/连接池/wake wrapper 仅在非 local provider 注册后才有意义。
- 坑（仍有效）：表结构存在 ≠ 功能就绪——现在功能本体已建，但消费者尚未切换。

### G-2 ✅ 插件 guest 桥 3 个 hook 域已桥接
- `permission.evaluate` / `aisdk.sdk` / `aisdk.language` 已在 `oc-core/src/plugin_apply.rs` register_guest_hook + `oc-plugin-host/src/host/host.js` 白名单落地；`ctx.permission.hook` / `ctx.aisdk.hook` 可用；真 bun guest 集成测试覆盖。
- 已记录偏差：aisdk 句柄 copy-in/out（guest 返回的 JSON sdk 成为新 opaque 句柄；language 句柄始终透传）；guest 显式 `sdk: null` 视为保留原句柄（serde 无法区分 absent/null，已在 codec 注释记录）。

## 中优先级

### G-3 ✅ KV.scan 已移植
- `Tx::kv_scan` + `Database::kv_scan`（`oc-core/src/database/schema.rs`）：prefix 下界 + prefixEnd 上界（镜像 ref kv.ts:85-90 含其怪异边界）+ 严格 `>` 游标 + limit+1 探测 + 1..1000 钳制；5 个新单测。插件侧 scan 面本已存在（hooks.rs DatabasePluginStorage::scan）。

### G-4 ✅ worktree 策略服务已移植
- `oc-core/src/project.rs` worktree_strategy 模块：Strategy trait（create/remove/list）+ GitStrategy（对齐 ref worktree/git.ts，--detach 保留）+ 注册表（discovery 反序）；`claim_worktree`/`remove_worktree` 移植 ref setWhere 语义（发现仅认领 strategy IS NULL 行，replace:true 例外）。
- oc-server worktree create/refresh/remove 全部改经注册表，wire 形状保持。插件注册自定义 strategy 仍不支持（ref 经 plugin editor；本注册表为主机侧最小面）。

### G-5 ✅ 插件运行时配置变换链已移植（agent state 面）
- `oc-core/src/state_transform.rs`：AgentState/Editor/Transform + run_agent_transforms 对齐 ref state.ts:172-194（注册顺序、grouped 失败→整组禁用+从新候选重启、ungrouped 失败→传播）；plugin_apply 快照后运行，失败隔离。
- 有意偏差（docs/design-g5-state-transform.md）：eager 重建、仅限 state transform 组、host-side only（guest 桥禁止阻塞 IPC）。

### G-6 ✅ TUI composer 多 tab UI 与 `@`-reference 补全已移植（忠实移植）
- composer 三 tab picker（Subagents/Shell/Terminals）：注册门控、每 tab 提示/空态/wrap 语义、每 tab 键位 enable 层（修复 Shell tab 上 return 误解析为 subagent.select 的真 bug）；body 窗口滚动+选中居中（ref scrollbox 对齐）。
- `@`-补全新增 Reference::Source 条目（skills→references→agents 顺序、前导空格描述、FileMention uri-dedupe、frecency bump）；`/help` 入口已核验可达（command palette + DialogKind::Help）。
- 已记录偏差（代码注释）：Shell 输出查看器打开时取一次（ref 每 1s 轮询）；reference 行按击键重挂载；无绑定的 ctrl+d 被吞不落穿；FileMention 以 name 为去重键（无 uri）。

### G-7 ✅（部分：generateObject 已移植；evaluation lane 有意不移植）
- `LLM.generateObject` → `crates/oc-ai/src/llm.rs`：强制合成 generate_object 工具 + ToolChoice::named 走既有 route/protocol 管道；ref 一致的错误文案；JSON-Schema 子集校验器（比 ref dynamic 模式更严，无新依赖）；取首个匹配 tool call（ref find 语义）。
- ℹ️ experimental evaluation lane（evaluation/evaluation-client/system-one + 3 recording 目录）有意不移植：ref 主流程不消费（G-17 同类裁决）。

### G-8 ⚠️→✅（可落地部分已落地；三包维持有意不移植）
- ✅ WS 代理：`oc-ai/src/route/transport/proxy.rs`——scheme 优先级选代理（wss/https→HTTPS_PROXY/ALL_PROXY 等）、NO_PROXY（* / 点后缀 / 端口钉定，含隐式默认端口 443/80 的 effective-port 匹配）、手写 HTTP CONNECT（15s 预算、8KiB 上限）、隧道流交 client_async_tls_with_config；真桩代理 WS 端到端测试。https/socks 代理 URL 以明确错误拒绝（CONNECT-only 范围）。
- ✅ `OPENCODE_DISABLE_FFF`（ref truthiness 解析，honored——钉定纯 ripgrep 路径）与 `OPENCODE_SIMULATE`（boot 一行 "not supported in this build" 提示）已成为功能性开关（server_process_env.rs + boot.rs）。
- ℹ️ 有意不移植：`@opencode/plugin-browser`、`@ff-labs/fff-*`（纯 ripgrep 路径即目标行为）、`@opencode/simulation`、gitlab `discoverWorkflowModels`、Windows `ShellSelect.configured`（本构建非 Windows 目标）。

### G-9 ✅ Mercurial（hg）vcs 管道已移植
- oc-server handlers/project.rs：hg 适配器对齐 ref plugin/vcs/hg.ts——HGPLAIN、`status -0` 映射（A|?→added，R|!→deleted）、working diff、branch 模式经 `ancestor(., default)`（default 分支上返回 []）、committed/显式 base → 503 DiffError（ref 文案）、`hg cat` 合成删除补丁 + `?` 合成新增补丁、patch 预算、截断时仅丢弃尾部不完整 chunk（ref splitGitPatch 语义）；`/vcs/base` on hg 返回 200 null（ref vcs.ts:198-201）。
- 桩 hg 脚本 wire 测试（parity_vcs_wire::hg_vcs_wire）覆盖全部模式。
- 已知偏差（代码注释）：scope 恒为项目 canonical（与既有 git 路径一致的简化），nested location 不按子目录裁剪。

## 低优先级

### G-10 ✅ PTY WS 无效 UTF-8 已对齐为丢弃
- `oc-server/src/handlers/pty.rs`：decode_pty_input 镜像 ref protocol.ts:30-36 fatal-decode-or-drop——整 chunk 丢弃（有效前缀也不转发）；replay 分块按字符边界切 64KiB 帧。

### G-11 ✅ CORS `oc://renderer` 已改为前缀匹配
- `oc-server/src/middleware.rs` starts_with("oc://renderer")（与 ref cors.ts:15 逐字节一致，含其对 `oc://renderer-evil` 的同等宽容——parity 优先）。

### G-12 ℹ️ `context_management.edits[].type` 硬编码 `"compact_20260112"`
- `oc-ai/src/protocols/anthropic_messages.rs:390,1791`。注意：入站侧同时校验并 fail-fast（非该类型直接报错），**不会静默丢弃**；但新增 edit 类型需手改两处。（本轮未动——ref 侧同样单一类型。）

### G-13 ℹ️ oc-syntax 已知降级
- markdown 代码围栏无 injection 高亮（`oc-syntax/src/lib.rs:43-45` 明示）；csharp/kotlin/swift 单独 feature、extra 14 语言 `langs-extra` feature（体积上限 67.6 MiB 裁决 R-01b）。

### G-14 ✅（retention 可配置已落地；审计轨迹差异维持有意）
- ✅ `experimental.file_retention.cleanup_interval`（毫秒；absent/0 → 每小时默认，0 守卫防 tokio interval panic）+ 调用方 Duration 透传（`cleanup_with` / `cleanup_data_root_with_retention`）；EngineOptions + boot 播种。oc-schema/config/normalize 按既有 knob 模式。
- ℹ️ event audit：维持 bus registry + SSE manifest（ref JSONL 落盘）——架构性有意决策，本地审计轨迹差异已文档化。

### G-15 ✅ 死依赖已清理
- oc-core：`once_cell`、`glob` 已移除；oc-cli：`dirs` 已移除。勘误：`unicode_width` 实际从未声明（原审计条目过时）。

### G-16 ℹ️ 长期漂移风险（维护坑，未变）
- `oc-schema` 手写 serde 类型与 ref Effect Schema 平行维护，无生成器，靠 goldens 防漂移——最大长期漂移面。
- provider 折叠架构：新增厂商需同时改 `provider_catalog` static_entries 与 `variant.rs:1089-1123` 映射表两处；建议与 ref `providers/index.ts` 厂商清单建立自动对照测试。
- `/openapi.json` 为 sha256 钉死的静态文档；ref 升级后需人工走 baseline-bump 流程（有 sha 断言兜底）。

### G-17 ℹ️ 有意省略（正确决策，无需补；未变）
- ref 死代码：`mcp/resolve.ts`、`plugin/inventory.ts`；ref 的 `ignore` 依赖在其源码中也无 import。（勘误：`debug/redact.ts` 并非死代码——ref 调用链完整 `cli/src/index.ts:41 → handlers/debug/config.ts:8,16`，见第五轮 G-52。）
- Web/GUI/云生态：web/desktop/enterprise/console/app/posts/storybook/identity/containers/function 等整包不移植（codemode/latex/merman/simulation 的**功能本体**已在 oc-core/oc-tui 内实现或按上文明示处理）。
- rust-extra 超出 ref 表面的部分（`upgrade -m local-file|github`、completion、`--force`、run 的 TTY 权限提示、serve hostname 解析报错、`$PWD` 陈旧值守卫等）均为防御性收紧且有注释。

## 本轮 burndown 之外的已知残留（后续工作）
- G-1 consumer 迁移：tools/handlers（read/write/edit/shell/glob/grep/patch、mcp/stdio、pty）尚未切换到 Environment Files 服务（服务本体与接线已就绪）。
- G-4 插件注册自定义 worktree strategy（ref plugin editor 面）。
- G-6 Shell 输出查看器 1s 轮询（现一次性拉取 + 64KiB tail）。
- G-8 https/socks 代理 URL 支持（TLS-to-proxy / SOCKS 握手）。

---

# 第三轮全量对比审查（2026-10-30）

以下 G-18..G-40 均经两侧源码二次取证确认仍存在。风险排序：🔴 高 / 🟡 中 / 🟢 低。

## 🔴 高

### G-18 ✅ bedrock-converse thinking block_binding 默认值缺失
- ref `packages/ai/src/protocols/bedrock-converse.ts:485-503,562-563`：Claude 5.1+ 注入 `adaptive + drop_block + anthropic_beta: thinking-binding-controls-2026-08-01`。
- rust `crates/oc-ai/src/protocols/bedrock/converse.rs:630-685`：thinking 仅从 provider_options 透传，无 block_binding/adaptive/drop_block/anthropic_beta 任何痕迹（同仓 `anthropic_messages.rs:1493-1518` 已有 `applyThinkingBindingDefault` 能力，未接到 converse 路径）。
- 后果：Bedrock Claude 5.1+ 会话中 system 提示变更 → `Invalid signature in thinking block` 硬错误，长会话断流。fixture `test-fixtures/recordings/bedrock-converse-thinking-binding/` 已拷入但无测试引用（死 fixture，可直接补测试）。

### G-19 ✅ bedrock-mantle-messages 路由整条缺失
- ref `packages/ai/src/providers/amazon-bedrock-mantle.ts`：anthropic-messages 协议跑在 mantle host（含 effort 更新、haiku system 降级、签名 thinking 回放，4 组 recorded fixture）。
- rust `cache_policy.rs:44` 白名单留名，但 `ProtocolFamily`（provider_catalog/facade.rs:269-272,391-394）仅有 BedrockMantleChat/Responses，无 anthropic-messages-on-mantle 变体；4 组 fixture 同样已拷入无引用。
- 后果：Bedrock Mantle 上 Claude 的主推理通道不可用。**修复优先级最高（与 G-18 同组）。**

### G-20 ✅ IDE/编辑器实时集成组缺失（高）
- **WebSocket 集成**：ref `tui/src/context/editor.ts:107-376`（`~/.claude/ide/*.lock` 发现、`CLAUDE_CODE_SSE_PORT || OPENCODE_EDITOR_SSE_PORT`、JSON-RPC selection 推送、`x-claude-code-ide-authorization`）。rust 全仓 grep 零命中；仅有 Zed sqlite 轮询（editor_zed.rs:83-95）+ `$EDITOR`（editor.rs:6）。守卫基线（tools/guards env_ref.txt:34）记录了该 env 但无实现。
- **terminal.suspend 死键**：键位表绑定 ctrl+z（oc-tui-core/keymap/table.rs:786），但 oc-tui 无任何 SIGTSTP/suspend 分发臂（ref app.tsx:1120 有）。按键被消费无动作。
- **session.environment 不上报**：ref app.tsx:489-499 就绪后 PUT；rust 仅 .gen 生成代码存在（methods.gen.rs:3633），无调用方。
- 三项同组：VS Code/JetBrains 编辑器上下文（@-reference 选区）完全不可用。

### G-21 ✅ tui.command.execute / tui.session.select 事件未处理
- schema/总线/客户端侧齐全（oc-schema/events.rs:306-362、oc-client/bus_event.rs:144,409、bus/registry.rs:362-367），但消费侧仅 `tui.toast.show`（store.rs:10853）。ref app.tsx:1253-1274 将其分别接到 keymap.dispatch 与 route.navigate。
- 后果：插件经 `ctx.event` 发的命令/路由定向指令被静默丢弃——插件 API 契约级缺口。

## 🟡 中

### G-22 ✅ ProviderContext/路由 hook 不兼容防护缺失（主请求路径）
- ref `model-request.ts:304-318`：每请求检查 model.request hook 改写 baseURL 是否与含 compaction part 的 opaque window 兼容，不兼容即 die。
- rust 仅压缩路径有防护（compaction.rs:434-442）；主路径 model_request.rs:747-788 应用 RoutePatch 后无任何 provenance 检查（grep 0 命中）。
- 后果：改写 baseURL 的 hook 可把加密 compaction window 发往另一部署，产生不可回放历史且无 die 信号。

### G-23 ✅ session.generate hook 定义但零调用（含死注册）
- rust `model_request.rs:653-673`：`RequestKind::Generate` 落入 `dispatch_session_context`；`dispatch_session_generate`（hooks.rs:891）全仓 0 调用点。
- 更糟的是已有 3 处注册（engine/mod.rs:2127,2255、tool/patch.rs:561）——注册了永不派发的 hook。需追查这些 handler 期望的触发点，可能已是功能性 bug 而非纯接口缺口。

### G-24 ✅ 插件 host 域缺失：websearch / vcs / worktree transform + integration methodImplementation
- ref `plugin/host.ts:468-490`（vcs get/base、websearch providers/query/reload/transform）、`:631`（integration 方法注册通道）。
- rust oc-plugin-host hook 域仅 `session|chat|tool`（context/mod.rs:120-132）；grep websearch/vcs/worktree 零命中。oc-core 有内部 OAuthImplementation 注册表但无插件暴露通道。与 G-4 残留（worktree strategy）同族。

### G-25 ✅ pty connect-token Origin 校验忽略端口（未复用既有实现）
- rust `oc-server/handlers/pty.rs:191-207`：Origin 与 Host 均剥端口比较；ref（middleware.ts:66）含端口精确比较。auth.rs:210-227 的 `origin_host` 本就是端口敏感实现但未复用。
- 同主机不同端口页面可通过检查；利用受 ticket 单次+60s TTL 缓解。修复即改为复用 `is_allowed_request_origin`。

### G-26 ✅(bounded) reqwest 0.12/0.13 双栈证书验证不一致
- Cargo.lock:3213/3255 双版本共存（有意，oc-core/Cargo.toml:44-50 有注释）。两栈同走 rustls 0.23（aws-lc-rs provider），但 0.12 栈用 `webpki-roots`（Cargo.lock:3243）、0.13 栈用 `rustls-platform-verifier`（:3277）。
- 后果：企业自签 CA/中间盒环境下两栈连接行为不一致（0.12 栈拒绝系统信任证书）。与 G-8 同属传输层债务。

### G-27 ✅(bounded) ACP vendored patch 无上游 drift 检测
- third_party/agent-client-protocol（crates.io 3.3.0 去 preserve_order 单点改动；ref 用 @agentclientprotocol/sdk 1.6.0——版本代差本身也是对齐风险）。CI（ci.yml/runtime-gates.yml）仅有 OpenAPI 与 vendored .scm drift 检测，无 ACP 上游 diff 检测。
- 上游发版需手工重打 patch，否则 feature unification 静默翻转全 workspace serde_json Map 顺序。属 G-16 族，单列因有具体 CI 补法。

### G-28 ⚠️ 插件 TSX 渲染为有界 shim（兼容性降级）
- rust 有 JSX 面（host.js:1857-1900 @opentui/solid shim 的 createElement/setProp/insert + supervisor.rs:1051-1111 ui.slot.register/router.register），但渲染树跨 IPC 时拍平为 `{title, rows, clickable}` 有界结构（supervisor.rs:964-966）。无任意组件树/markdown code-block renderer 注册面。
- 行式插件（sidebar 类）可跑；富 TSX 布局插件无法忠实渲染。ℹ️ 属有界化设计而非疏漏，但需作为插件生态 breaking change 显式声明。

### G-29 ✅(bounded) TUI 插件管理页：安装/更新/检查流无操作
- rust command_palette.rs:2905-2909 有插件列表+active 翻转+错误详情；但 `plugins.install | dialog.plugins.install | update | check` 显式 `Dispatch::Handled` 无操作（:3033-3047，注释 "registered-but-surfaceless"）。ref system/plugins.tsx 358 行完整管理 UI。CLI `opencode plugin add` 可绕过。

### G-30 ✅(bounded) 快照用 git2 且无 opencode.gitconfig 等价配置
- ref git.ts:23,28（`autocrlf=false` 的 opencode.gitconfig）、:535（`-c core.quotepath=false`）。rust snapshot.rs:248-278 用 libgit2，grep gitconfig/autocrlf/quotepath 零命中——还会不受控地继承宿主机 `~/.gitconfig`。
- 后果：CRLF 检出工作区快照 diff 整文件误报；非 ASCII 路径引用语义不一致。低成本修复：git2 `Repository::config` set_str。

### G-31 ⚠️ 文件模糊搜索无持久索引 + VCS 根 100k 截断
- rust oc-server/handlers/fs.rs:254-330：每请求 spawn_blocking 全量 walk；SCAN_ENTRY_BUDGET=100_000 统一上限（:319-323 注释自认 accepted deviation）。ref filesystem/search.ts:72：VCS 根不限量（MAX_SAFE_INTEGER）+ fff/fuzzysort 内存索引 10s 刷新。
- 后果：超大仓库第 100k 条目静默截断；大仓库逐字符补全延迟显著。

### G-32 ❌ v1/legacy schema 未移植
- oc-schema/src/lib.rs:80-82 TODO 仍在：legacy-event、v1 session 事件、models-dev refresh、rpc.ts、pty-ticket、durable-event-manifest。仅影响旧数据互操作/回放，纯 v2 面无影响。

## 🟢 低

### G-33 ✅ shell 输出截断上限 config 失联
- schema 能解析 `tool_output.max_lines/max_bytes`（oc-schema/config/tool_output.rs:10-15、normalize.rs:550），`ToolOutput::configure` 也存在（tool_output.rs:209-214），但 engine 从不调用（engine/mod.rs:488 仅 `ToolOutput::new`）——config 键解析后全程无人消费；shell.rs:382-384 直接用编译期常量。默认值与 ref 一致，仅显式配置失效。修复即 boot 时 `configure(Limits::from_config)`。

### G-34 ✅ session.new_location (launch/inherit) 未消费
- 字段已有（oc-core/cli_config/mod.rs:192-193,405）、配置对话框可编辑（dialog_config.rs:193），但 command_palette.rs:2192-2197 `"session.new"` 无条件回 Home。ref config/new-session-location.ts：inherit 回当前 LocationRef。

### G-35 ✅ composer 运行态 knight-rider 渐变 spinner 缺失
- ref ui/spinner.ts:272-368（createFrames/createColors）+ prompt/index.tsx:1892 `<spinner interval=40>`；rust composer 域无帧生成器（render.rs:74-88 仅文本 hint）。mini 面有 spinner（lifecycle.rs:552-674），主 composer 无。纯视觉反馈。

### G-36 ✅(bounded) mini demo 剧本覆盖 6/13
- rust mini/demo.rs:29 KINDS 6 项；ref demo.ts:34-48 13 项（缺 motion/shell/write/edit/patch/subagent/question）。演示与回归覆盖面缩减。

### G-37 ✅(bounded) session.message.get O(n) 全量扫描
- oc-server/handlers/message.rs:156-165：拉全部消息再线性 find。正确性无误，长会话性能退化；建议走 store 按 id 直查。

### G-38 ℹ️ OTel 默认关闭
- oc-server/Cargo.toml:11-17：`otel` feature 无 default（发布构建不拉遥测依赖）。可观测性弱于 ref（ref 默认开），属发布决策。

### G-39 ℹ️ 剪贴板写仅 OSC52（有意）
- clipboard.rs:208-236：host 写臂恒 Unsupported，注释明言 "v2 parity is the OSC 52 terminal write"；读臂健全（pbpaste/xclip/xsel/wl-paste + OSC52 查询）。不支持 OSC52 的终端复制静默失败。

### G-40 ℹ️ 记录性差异（无行为风险）
- `x-opencode-build` 响应头（info.rs:17，ref 无）——golden 对比时需归一化。
- 空分页 cursor 显式 `null` 而非省略键（message.rs:125,132）。
- Markdown 语法 injection 缺失与语言 feature 门控见 G-13。

## 勘误（第三轮初报经核验撤销的条目）
以下初报缺口核验后确认**已实现或原本不存在**，勿再列入待修：
1. **桌面/权限通知**：notifications_plugin.rs 是 ref notifications.ts 完整移植（permission.asked/form/execution 事件 + os_notify OSC9/777/99 + bell 回退，app.rs:6857-6871 接线）。
2. **npm registry 插件安装链**：oc-plugin-host/context/install.rs 经 npm CLI 子进程支持 registry + git specifier（`npm view` 探测、frozen 安装、缓存布局）；与 ref 的 arborist/pacote 仅机制不同。
3. **插件 storage KV**：StoragePort 四方法全分发（supervisor.rs:55-70,374-375）+ DbStoragePort 绑定（oc-server/plugins.rs:371-378 → hooks.rs:1281-1342，`plugin:<hex>:` 命名空间）。旧 TODO 注释已过时。
4. **wellknown/credential 联动**：HostWellknownSource（integration/wellknown_config.rs:79）经 engine/mod.rs:884-885 注入；600s 刷新循环 + CREDENTIAL_SWITCHED/INTEGRATION_UPDATED 事件触发均在（engine/mod.rs:563-580）。NoWellknown 仅测试默认。
5. **formatter 命令执行**：BuiltinsFormatter 真实执行（formatter.rs:793-875，26 个内置探测）且生产已接线（engine/mod.rs:1153）。
6. **`--session-output` 调试流**：ref 侧本就无此实现，初报无依据。
7. **bus replay 键序敏感**：幂等跳过按 event.id + seq（bus.rs:984-997），不比较 payload；workspace serde_json 不开 preserve_order，键序无关。
8. **pair --url 校验时机**：handler 侧 validate_pair_url 完整（misc.rs:246-260，RB-03 对齐 ref），仅 parse/handler 时机差，行为无影响。

另有 1 条修正归类：**MCP stdio 本机 spawn** 并非新缺口——environment/ 服务本体已移植（G-1 ✅），缺的是 mcp/client.rs 的 consumer 接线，已登记在 G-1 残留清单；当前唯一 driver 为 local，行为与 ref 等价，接入非本地 workspace driver 前必须先切线。


---

# 附：踩坑记录与现行制度（自 a26e145 重写前版本恢复，2026-10-30）

> a26e145 将本文件从 520 行登记册重写为 G-1..G-17 摘要时移除了原 §2/§3（踩坑记录与制度速查）及 §1/§4-8 历史发现表（RB/RC/RD/RV/RW/AUD/RE/RF 系列，绝大多数已闭环，历史全文见 `git show db21631:docs/GAPS-AND-PITFALLS.md`）。以下两节为**现行有效的事故规则**，原样恢复。

## 踩坑记录（事故 → 根因 → 现行规则）

### 2.1 资源与调度

| # | 事故 | 根因 | 现行规则 |
|---|---|---|---|
| 1 | **多子 agent 同时编译 → CPU 饱和 → 共享主机的 newapi 在线服务卡死** | 本机与在线服务（newapi、49374）共享 16 核/22G；并行 lane 各自 cargo build 叠加无上限 | 一切本地构建/测试经 `tools/dev-gate.sh` 单入口：强制 eternal130 身份 + systemd transient scope（CPUQuota=400% 四核硬顶、MemoryMax=10G、TasksMax=512）、1 分钟 load≥6 拒启动；lane 提示词必须声明走 dev-gate |
| 2 | root 直跑 cargo，三次属主污染 | root 产物属主混入用户目录 | dev-gate 强制降权身份；CI 例外（不占本机） |
| 3 | 共享同一检出目录的两个 lane 互相删文件 | 写所有权无隔离 | lane 写隔离=每 lane 独立 git worktree（`.worktrees/`）+ 串行合并回 main；只碰自己 scope 的文件；收尾前重查 `git status` |
| 4 | /tmp/opencode 被整体清空，5 条 lane 未提交工作全失 | API 配额耗尽杀 lane 后，外部清场连证据目录一起清 | lane worktree 移入仓库内 `.worktrees/`；里程碑 WIP 提交强制（小步提交、事后 squash）；/tmp 顶层只用 PID 唯一名 |
| 5 | SCAN-EF 扫描腿（并行编译 + 探针连发 release 二进制）严重拖慢同机生产的 v2 服务，被用户击杀 | 验证负载无节制 | 生产尊重制度：**单 lane 串行**；扫描腿小批量带暂停；重型腿（E2 TTI/soak/电池）需用户明示或错峰；派发前看生产健康信号 |
| 6 | `target/debug` 无界增长至 **476G**（debuginfo 主导 + 增量编译无指纹 GC），磁盘 99% 满 | cargo 增量缓存只增不减、多 profile 叠加 | 2026-10-23 清理 486G（保留 release 3.2G）；建议日常 `CARGO_INCREMENTAL=0` 或周期 `cargo clean`；无 worktree 残留（已核） |
| 8 | **incremental=false 已生效但磁盘仍持续上涨**（2026-10-27 复盘：主 target/debug 153G + 5 个 lane worktree 各自全量 target ≈51G；incremental 目录实为 4KB，非根因） | ① worktree 天然隔离 → 每 lane 独立编译全依赖图，`incremental=false` 管不到；② cargo 从不 GC target 内陈旧产物（历史 lane 残留/多版本 deps）；③ lane 完成后 target 无人回收 | 三策：① **lane 合并后立即删该 worktree 的 target**（保留分支与 worktree 本体亦可先只删 target；删前过清理门=fuser 无打开文件+30 分钟 mtime 规则）；② worktree 构建可改用 `CARGO_TARGET_DIR` 指向主 target 复用依赖产物（注意指纹翻转抖动与 cargo 文件锁串行化，需 dev-gate 协调）；③ 主 target/debug 定期（≥两周或 >100G）`cargo clean -p` 粗清或整清 |
| 7 | API 配额窗口耗尽（92%）在半夜杀 lane | 配额无监控 | 定时唤醒 + 断点复活模式（WIP 落盘后 lane 死亡也可 revive） |

### 2.2 进程与终端安全

| # | 事故 | 根因 | 现行规则 |
|---|---|---|---|
| 1 | **测试盲杀信号误杀系统服务**（生产事故） | 测试按 PID 名/号发信号，撞上复用 PID 的系统进程 | safe_signal 结构性修复：pidfd 系统调用 + boot_id/进程 starttime 世代 sidecar + 外来记录 cmdline 精确校验；全仓禁裸 PID 发信号；组杀=活 leader 钉住+/proc 扫描+逐成员校验；测试只用自有子进程句柄（process_safety guard 常设） |
| 2 | **扫描腿裸启 TUI 二进制锁死用户终端**：raw mode + alt screen 缓住真终端，两个 TUI 同写一屏→持续乱码、ctrl+c 被吃、被迫重启生产 v2 | E2 进程拉起腿未套 PTY harness，直接 `cargo run` 冒屏 | TUI 二进制任何 lane 只准在 vt/PTY harness 内或 stdout/stderr 重定向 + no-tty 模式启动；tmux/终端矩阵探针只碰自建会话；OSC/转义探针只写自有 PTY |
| 3 | **测试进程泄露**：孤儿 rig/电池进程、stale ref-bun 进程（GATE3 一轮杀 12 个）、lane 死后后台矩阵孤儿化（rig 二进制消失、配对进程残留） | 后台长跑无归拢、teardown 不完整 | 长验证一律前台分块（≤45 分钟/块）；rig teardown 卫生项常设；GATE 轮清扫；清理门=mtime<30 分钟视为在用（两次误踩活动目录后立规） |
| 4 | rig 使用陈旧二进制产物，两次被差分器抓获 | lane 换代码未重建 rig 缓存 | lane 收尾强制重建检查（已知陷阱清单） |
| 5 | **孤儿 `sleep 300` 占住 dev-gate 全局锁 10+ 分钟**（2026-10-25，lane 收尾 watchdog 脱离父进程后持 flock 睡眠，全部构建排队静默堵死；宿主后台 shell 任务亦被清理丢失） | lane 派生的 sleep/watchdog 未在收尾时回收；flock 无超时 | lane 收尾自查自派生后台进程（派发模板新增必查项）；构建排队异常时先 `fuser /tmp/opencode/dev-gate.lock` 查持锁者；重试循环勿依赖宿主后台 shell 存活 |

### 2.3 宿主 / 会话生命周期

| # | 事故 | 根因 | 现行规则 |
|---|---|---|---|
| 1 | **60 分钟 location-TTL 收割器多次静默杀后台 lane**（含 9E 后台矩阵中断） | 宿主对后台会话的位置 TTL 驱逐 | 长任务=前台分块 ≤45 分钟 + 追加式日志 + 每 6 轮 WIP 提交（断点可续）；**单次工具调用硬上限 45 分钟**（一次 80 分钟单调用险触 TTL） |
| 2 | 完成后的会话收到排队消息 → 幽灵重启（M6 幸无实害） | 排队投递无终态检查 | 禁止向已完成会话投递；干预一律 kill + 新 lane |
| 3 | H-1：复活子代理的第二份报告永远到不了父级（completed→completed 不是迁移） | 宿主缺陷，非本仓可修 | 子代理**禁止跑后台任务**（全部前台块）；父级 watcher 允许；完整分析见 docs/host-issues.md；候选上报上游 |
| 4 | 小时级后台睡眠被宿主收割（40 分钟级偶成） | 宿主对长睡眠的回收 | 级联短看守（50 分钟自续完成通知）+ 用户消息兜底 |

### 2.4 验证方法论

| # | 事故 | 根因 | 现行规则 |
|---|---|---|---|
| 1 | **陈旧 oracle 缓存掩盖真回归**：bun 全局缓存以 @opentui/core **0.6.1** 顶替钉版 0.5.14，病灶窗口内全部基准标错；"从未真绿"的提交穿门而过（双病灶之一） | oracle 缓存无版本/新鲜度证明 | oracle 三可证：版本硬断言（fail unless 0.5.14）+ 采集指纹（driver+scenario FNV-1a 双语镜像）+ always-refresh 终门；main.rs 自愈陈旧缓存、连续漂移硬错 |
| 2 | **"实现了 ≠ 链路接通"**（两案：F16 leader>e 审计标 done 实战失效；sidebar 遮罩有渲染无关闭键层，模糊键全进不可见输入） | 审计只证实现存在，未证链路端到端 | 交互面修复必须带 PTY/field 级验证；审计状态只是起点提示 |
| 3 | **双端差分 rig 五轮门只抓 fmt/goldens/flake，漏掉全部真实用户可见 bug**（omo-slim 侧栏、子代理切换、marquee、spinner 色、thought gutter 五案）→ 用户裁定退役 | 参照平价看不见用户插件内容；语料 vanilla-stub；时序节奏与色 token 绑定从未步进验证 | 差分测试 2026-10-17 退役；扫描循环接棒（后按裁定于 FIXW2 终止）；golden 降级为回归套件；常设仪器补：真实插件沙箱 + 时序 + token 覆盖 |
| 4 | 金帧字节自洽放过"屏幕组合模型"错误（错 tab 模型、错 boot 路由都活到 beta.7 实战） | 金帧证明渲染保真，不证明组合/路由模型 | 交互模型类（路由/tabs/列表拉取）必须做参照源码级对比（file:line），不许 golden 自证 |
| 5 | **合并静默回滚他 lane 成果**（C lane 自动合并回滚 B 的 availability 折叠、删测试；E2 被回退数个版本无人察觉，M0–M5 带伤建设） | 三/四方自动合并吃掉语义；rig 上差异被误读为"其他 lane 在飞" | 合并裁决必须列表级核对每个合并 lane 的删除清单；证据优先方整文件恢复；每 lane 合并后跑受影响 crate 测试 |
| 6 | **幽灵提交**两案：lane 报"已提交"而分支仍在 base | lane 自述无凭据 | 派发模板要求最终报告贴 `git log -1 <branch>` 落地证明；缺证明=未完成 |
| 7 | **无测试合并**：DERIG 推送未跑测试，main 的 parity 构建断一天 | 合并无门 | 每次 main 合并至少受影响 crate 构建+测试后再推，无例外 |
| 8 | merge 后必须补 bun 内差分冒烟（纯 cargo 门看不见 JS 侧破坏，O2a×O2b 融合后 driver.ts 语法断裂存活） | 纯 Rust 门对 JS 侧盲 | harness 相关合并加 bun 冒烟 |
| 9 | **子代理"默认测试通过"假象 + 过滤指令吞失败详情**：首轮 cargo test 用了过滤/未带 --no-fail-fast，失败被折叠成汇总行，失败原因不可见，被迫第二轮重测定位，拖长 lane 周期（2026-10-27 收口 session 多 lane 观察） | 测试报告只看退出码/末行汇总；过滤参数（--lib、name filter）把红测试藏进被滤掉的集合 | lane 验证纪律：① 首轮测试**不加过滤**并带 `--no-fail-fast`，拿到全部结果；② 报告必须**原样引用 `test result:` 行**（含 passed/failed 计数），禁止只写"green/通过"；③ 有失败时**同一轮**立即用 `--exact <name> -- --nocapture` 重跑单个红测试取回失败输出，不得先交报告再返工；④ dev-gate 输出被截断时读保存的完整输出文件而非重跑 |
| 10 | **测试证据随会话丢失 → 重复测试**：lane 报告只活在子代理会话里，宿主重启/会话回收后 orchestrator 或后续 lane 拿不到原始 test result 行，只能整轮重跑验证（2026-10-27 合并门曾整轮重测 5 crate，数十分钟；且 /tmp/opencode 有被整体清空的前科 2.1#4） | 测试输出没有落盘为持久文件；依赖会话内存或易失 /tmp | **一切测试运行必须 tee 落盘**：`tools/dev-gate.sh 'cargo test …' 2>&1 \| tee .test-logs/<crate>-<yyyymmdd-HHMM>-<lane>.log`（.test-logs/ 进 .gitignore、不随 /tmp 清空；merge gate 同规则）；lane 最终报告**必须给出日志文件路径 + 关键 `test result:` 行**；后续任何"是否已验证"的争议先读日志文件，禁止凭记忆重跑；日志含红项时连同基线证伪证据（基线 commit + 复现输出）一并落同一目录 |
| 11 | **plain `cargo test --workspace` 产生大面积假红**（2026-10-28 rv-burndown 收口：cargo test 直跑 oc-tui 数十测红——PTY/VT 套件 spawn 真终端+stub server，在无限流/无重试下时序全崩；同一棵树 nextest 4450/4450 绿） | 本仓 canonical 测试 harness 是 nextest（.config/nextest.toml：test-threads=4、全局 retry=1、be1/pty 家族 retry=2 + [profile.pty] test-threads=1 序列化）；plain cargo test 绕过全部降载与重试机制 | 全 workspace 验证一律 `cargo nextest run --workspace --locked --no-fail-fast`；PTY 家族另跑 `--profile pty` 序列化（镜像 ci.yml 两 job）；禁以 plain cargo test 的红作为回归证据，除非单测 --exact 定位复现 |

### 2.5 工程卫生

- 发布门 scoreboard：beta.13 起每修必配独立子代理门扫描，循环至全绿才发（GATE1–GATE5）。
- lane 派发模板强制项：English commits、分块调用（≤45 分钟）、worktree 隔离、写明 dev-gate。
- DNS 探针文件（/tmp/opencode/dns-probe.*）不是构建产物，勿清。
- 仓库根曾经堆积垃圾文件（`None`、`refprobe.pid`）——2026-10-23 清除；收尾自查 `git status` 为模板必查项。

---

## 现行制度速查（防复发汇总）

| 类别 | 制度 | 载体 |
|---|---|---|
| 资源 | 单入口构建/测试（配额+身份+负载闸） | `tools/dev-gate.sh` |
| 调度 | 单 lane 串行；重型腿需批准；生产健康前置检查 | 生产尊重制度（2026-10-18） |
| 终端 | TUI 只在 PTY/vt harness 内启动；探针只碰自有会话 | 终端隔离规则（2026-10-18） |
| 信号 | 全仓 safe_signal；禁裸 PID；测试用自有子进程句柄 | `tools/guards/process_safety.py` |
| 生命周期 | 前台分块 ≤45 分钟；WIP 提交；禁投已完成会话；子代理禁后台 | lane 派发模板 |
| 验证报告 | 首轮测试不过滤+--no-fail-fast；报告原样引用 test result 行；红测试同轮 --exact --nocapture 取回原因 | 本册 2.4#9（2026-10-27） |
| 测试证据落盘 | 一切测试 tee 到 .test-logs/<crate>-<时间>-<lane>.log（git-ignored）；报告给路径；争议先读日志禁止凭记忆重跑 | 本册 2.4#10（2026-10-27） |
| 写隔离 | 每 lane 独立 worktree；串行合并；删除清单核对 | 写隔离制度（2026-10-11） |
| 合并 | 受影响 crate 构建+测试后推送；落地证明（git log -1） | 合并门（2026-10-18） |
| oracle | 版本断言+指纹+always-refresh | `tools/diff`（opentui-rust） |
| 磁盘 | 增量抑制（仓库级 .cargo/config.toml incremental=false，2026-10-25 起强制）/周期 clean；清理门 mtime≥30 分钟视为在用 | 本册 2.1#6 |
| 验证 | 交互面必须 PTY/field 级；组合模型必须源码级对比；金帧=回归套件 | 方法论登记（ROADMAP §4） |

---

---

# 第四轮复审（2026-10-31，gap-burndown-3 收口后）

> 对本轮落地的 24 项修复做 3 通道只读代码级复审（oc-ai / core+server / TUI，逐项对照 ref 源码 file:line 取证）。裁决：20 项 CONFIRMED、4 项 PARTIAL（G-30/G-31/G-34 + G-19 的死校验）。无回归；以下为新登记缺口与半接线残留。

## 既有条目状态修正

- **G-34 ⚠️（降级为 PARTIAL）**：`home_location` 生产路径 write-only——`set_home_location`（store.rs:10545）唯一调用方是 `apply_new_session_location`（command_palette.rs:3360），`home_location()` 唯一读者是测试。Home 路由标签走 `footer_location`（boot location worker 设置），inherit 模式解析正确但无消费者，可见行为与 launch 相同。需把解析结果接到 home 路由的 directory 语义（ref app.tsx:718-737）。
- **G-30 ℹ️（补充）**：repo-local autocrlf/quotepath 钉定当前行为惰性——快照管道全程 raw blob/OID-map/treebuilder/手写 diff，从不咨询 git config；parity 为偶得（raw passthrough 即无 CRLF 误报）。钉定仅防未来 libgit2 操作。ref 另钉 7 个 key（longpaths/symlinks/fsmonitor/untrackedCache/manyFiles/index.version/threads，ref git.ts:23-33）未移植，当前行为中性。
- **G-31 ⚠️（补充）**：`is_vcs_root` 弱于 ref `location.vcs`——仓库子目录 location 与 hg 仓库在 rust 侧仍 100k 截断（保守方向）；handler 手里已有 `services.project.vcs`（fs.rs:260 绑定 `_services`）却用文件系统嗅探。
- **G-19 ⚠️（补充）**：`validate_config`（mantle_messages.rs:199-221）零调用——ref 的 bearer-without-key fail-fast（ProviderConfigurationError）永不触发，rust 静默 SigV4 回退；`auth/profile/region/credentials` settings 键在 oc-core ModelConnection seam（baseURL/apiKey/headers，facade.rs:123-136）无处解析（承自 converse lane 的既有模式）。
- **G-24（bounded 清单精确化）**：guest 侧 worktree host-op、`vcs.transform`/`vcs.reload`（dispatch 表仅有 websearch/worktree transform，supervisor.rs:356-369）、websearch `editor.default.set` 持久化（host.js:875-877 为 no-op stub）、worktree 注册表热重载（`AppState.worktree_strategies` 仅 boot.rs:439 赋值，`reload_after_update` 不重播种）。

## 新登记缺口（G-41..G-49）

### 🟡 中
- **G-41 ✅ 代理 URL 凭据被丢弃**：已修复——userinfo 解析保留（percent-decode），CONNECT 发 Basic `Proxy-Authorization`，SOCKS5 username/password 认证。
- **G-42 ✅ WSS_PROXY/WS_PROXY 未咨询**：已修复——WS 族优先于标准族（websocket-constructor.ts:43-47 语义）。
- **G-43 ✅ 无 loopback 目标绕过**：已修复——127.0.0.1/localhost/::1 恒直连。
- **G-44 ✅ NO_PROXY bare-host 语义偏差**：已修复——bare host 精确相等、`.`/`*` 前缀后缀匹配、空白+逗号分隔。

### 🟢 低
- **G-45 ❌ agent-client-protocol-schema 无 drift 守卫**：同为 vendored+同一 preserve_order 改动（third_party/agent-client-protocol-schema/Cargo.toml:103，同样经 [patch.crates-io] 钉定），acp_drift.py:39-40 的 VENDORED 硬编码只覆盖主包。扩 guard 即可。
- **G-46 ❌ dialog.plugins.error 仍无操作面**：command_palette.rs:3129 留在 surfaceless 组；ref plugins.tsx:224-228 有 error 详情动作。
- **G-47 ❌ IDE socket backoff 不复位**：连接成功后 `attempt` 不清零（editor_ide.rs:222-235 vs ref editor.ts:243）→ 抖动后重连恒等 10s；另空字符串 env 值阻断 fallback（ref `||` 视 "" 为 falsy）。
- **G-48 ❌ tool_output 限制 boot 一次性应用**：ref 经 ConfigEntryObserver 活跃重应用（config/plugin/tool-output.ts）；rust 改 `tool_output.*` 需重启。
- **G-49 ℹ️ 记录性/文档债**：spinner 帧率受 pump tick（50-100ms）约束而非 40ms 定时器（帧数学精确）；session.environment 每 session 单发（ref 效果重触发，PUT 幂等无害）；stdio.rs 模块头宣称 SIGTERM→SIGKILL 升级但实现为即杀（沿袭 rmcp 行为，措辞与 ref 2s 优雅等待不符）；project.rs:1103-1105 注释"插件 strategy 不支持"已过时（add_strategy 就在其下）；proxy.rs:367 注释"webpki 与 0.13 栈一致"在 G-26 后已失效（0.13 现走 platform-verifier；ws 链路留 webpki 为有意）。

## 复审判定汇总
G-18 ✅ / G-19 ✅(死校验残留) / G-20 ✅ / G-21 ✅ / G-22 ✅ / G-23 ✅ / G-24 ✅(bounded) / G-25 ✅ / G-26 ✅ / G-27 ✅(F1=新 G-45) / G-29 ✅(error 面残留) / G-30 ✅(惰性钉定) / G-31 ⚠️ / G-33 ✅(无热重载) / G-34 ⚠️(半接线) / G-35 ✅ / G-36 ✅ / G-37 ✅ / G-6 ✅ / G-8 ✅(G-41..G-44 已由 lane2 收口) / G-41 ✅ / G-42 ✅ / G-43 ✅ / G-44 ✅ / G-1 ✅(consumer 迁移仍开放)

---

# 第五轮全量对比审查（2026-10-31）

> 9 通道全量对比（session/配置/工具/infra/AI/TUI/server/CLI/schema-plugin，逐文件对读，总报告 `PORT_AUDIT_2026-10-31.md`）后，又对全部初报缺口做 4 通道只读二次取证。本节收录经核验 CONFIRMED 的新缺口（G-50..G-78）、既有条目订正与归类、以及撤销清单（勿再立项）。

## 🔴 高

### G-50 ✅ snapshot restore/capture 不保真文件模式与符号链接（已修：restore 按 tree entry filemode 恢复 symlink/可执行位，capture 以 read_link 入库 symlink 目标；commit 022c5ef）
- rust `snapshot.rs:489-527`：`restore_blocking` 过滤 Blob 后直接 `fs::write`，tree entry 的 filemode（100755/120000）被完全忽略——symlink blob（内容=目标路径）被还原为**含目标文本的普通文件**，可执行位丢失；capture 侧同样只以 blob 内容入库，元数据双向丢失。
- ref `git.ts:596-617` 经 `git checkout <tree> -- <file>` 原生恢复 exec bit 与 symlink。用户 revert 后仓库即损坏。
- 与 G-30 同文件邻接但语义独立（G-30=gitconfig 钥匙）。

### G-51 ✅ persistent-pty WS connect 缺 Origin 前置检查（已修：connect ticket 分支复用 origin_allowed，403 且不消费 ticket；commit d5f5c00）
- rust `persistent_pty.rs:275-283`：ticket 分支仅 `consume_ticket`，`:283` `let _ = headers;` 显式丢弃；同文件 `connect_token`（:247-249）反而有 `origin_allowed`——工具在手边未复用。
- ref `persistent-pty.ts:110-115`：`isAllowedRequestOrigin` 不通过即 403 且不消费 ticket。修复即 connect 分支复用 `origin_allowed`。
- 非 G-25 重复（G-25 是 pty.rs connect-token 端口剥离，已修）。

### G-52 ✅ `opencode debug config` 脱敏弱于 ref（已修：secretName 全量对齐 + URL userinfo/query-secret 整体打码 + headers 子树传播；commit 45d7765）
- ref `handlers/debug/redact.ts:2` secretName 含 `credential|private.?key|authorization$|cookie$`；`:11-17` URL 凭据/query-secret 整体打码 + headers 子树传播脱敏。调用链完整（index.ts:41 → config.ts:8,16）——**订正 G-17 原"redact 是 ref 死代码"断言（已改）**。
- rust `oc-cli/handlers/misc.rs` `is_secret_key`（~:472-476）仅 `apikey/api_key/secret/token/password` contains；无 URL 检查、无 headers 传播。可能明文打印 ref 会遮蔽的密钥。

### G-53 ✅ skill 工具输出格式错位（已修：prepare/prompt 共享 toModelOutput 信封 + 采样路径 .slice(0,10)；commit acf2e09）
- ref：`tool/plugin/skill.ts:38` content = `Skill.prepare` → `toModelOutput`（skill.ts:36-53）= `<skill_content>` 信封 + `fs.scan` 采样**路径** `.slice(0,10)`。
- rust `tool/skill.rs:24-47`：`prepare()` 把目录**全部文件内容**拼接（非递归、无 10 上限、无信封）；执行器 :172 调用之。信封实现已存在（`prompt_text` :55-82）但仅 prompt 附件路径使用。大技能目录会撑爆上下文。

### G-54 ✅ run 非交互：非 question 表单取消退出码 0（已修：cancel_form 非 continuing 分支 record_exit(1)；commit 2f0e21a）
- ref `run/noninteractive.ts:203-204`：`formCancelled` 非 v1 → `process.exitCode = 1`。
- rust `run/noninteractive.rs:334` 仅置 `form_cancelled`；该标志在 :672-685/:1100/:1119/:1135 被用作**跳过**错误路径的条件，最终 exit 0。脚本按退出码判断 run 成败会漏检。修复：cancel_form 非 continuing 分支 `record_exit(1)`。

## 🟡 中

### G-55 ❌ shell create 缺省 cwd 继承服务器进程 cwd
- rust `shell.rs:562` `unwrap_or_default()`，`create_invocation` 仅非空时 `current_dir`（:648-650）。工具路径安全（tool/shell.rs:165-168 显式覆写）；**生产命中点：HTTP `POST /api/shell`**（oc-server/handlers/shell.rs:40-52 直接透传可能为 None 的 body.cwd）。ref `shell.ts:265` `input.cwd ?? location.directory`。修复：handler 侧用 location 兜底。

### G-56 ❌ 凭据/wellknown 变更不驱动 config 重载与 location 失效（勘误 #4 半边修正）
- 刷新循环与 HostWellknownSource 接线属实（engine/mod.rs:570-596、:900-903）——勘误 #4 只对了这一半。缺的半边：①全仓 `request_reload` 生产调用点为零（仅 WatchedConfig 自身文件 watch 驱动，config.rs:655,671）；②credential/integration 事件不触发 location invalidate（ConfigReloadBridge 只挂文件 watch，boot.rs:113-140）——已 assemble 的 location 持旧 wellknown 快照直到其他原因失效；③boot 的 watch_loader 无 `.with_wellknown`（boot.rs:241-266）。
- ref `config.ts:306-322` 订阅 Credential.Switched / WellKnown.Updated → `reload()`；10 分钟刷新后亦 reload（:323-341）。与 G-48 同族不同面。

### G-57 ❌ 删除非 active 凭据多发 `credential.switched(credentialID:null)`
- rust `integration/credential.rs:331-360`：非 active 分支返回 `Some((Some(integration), None))`，发布门只判 integration——非 active 删除也发 Switched。oc-server/handlers/credential.rs 同病。
- ref `credential.ts:268-269,314-319`：`{switched:false}` 只发 updated；仅删 active 才发 Switched。修复：发布门加 switched 判定。

### G-58 ❌ Session.create 项目解析仅目录身份（插件 API 命中点）
- rust `store.rs:604-609` project 缺省回落 `hash("directory:…")`。HTTP/命令/transfer 主路径均显式传 `services.project`（完整 Resolver，engine/mod.rs:872-887）；**生产命中点：插件 `session.create`**（oc-server/plugins.rs:1300-1313 `CreateInput::default()` → project=None）。ref `session.ts:271` 无条件 `projects.resolve`（含 realpath + VCS 发现）。

### G-59 ❌ 工具输入校验为 JSON-Schema 子集，MCP 外部 schema 弱校验
- MCP schema 有注册并校验（mcp/registry.rs:1749 → tool/registry.rs:383 `validate_input`），真实缺口是子集语义：rust `registry.rs:88-232` 仅 type/enum/min/max/required/items 等，pattern/oneOf/$defs 引用等静默放行。ref `runtime.ts:107-113,251-258` 经 jsonSchema() 全量 draft-07/2020-12 转换校验。坏参数穿透到 leaf。

### G-60 ❌ list_mcp_resources 模型内容固定占位文本
- rust `tool/mcp_resource.rs:213` content 恒 `"Listed MCP resources"`；ref `runtime.ts:57,259-264` normalizeContent → stringify(catalog) 全文。模型在直接对话路径拿不到目录内容（output JSON 仍正确）。

### G-61 ❌ CLI auth login 无 target 时缺交互集成选择器
- rust `oc-cli/handlers/auth.rs:278-284`：无条件报错 exit 1。ref `auth/login.ts:46-110`：TTY 下 `selectIntegration`（MCP/Popular/Services 分类、connected 标记）。

### G-62 ❌ CLI plugin check/update 不覆盖 TUI(npm) 插件
- rust `oc-cli/handlers/plugin.rs` check（:426+）/update（:478+）只调 server 运行时；`configured_tui_plugins` 仅 list 使用。ref `plugin/inventory.ts:52-78` TUI 项走 `npm.check`；`plugin/update.ts:29` 走 `npm.update`。CLI 层缺口，与 G-29/G-46（TUI 管理页）不同层，交叉引用。

### G-63 ❌ persistent-pty connect 协议分歧（404 vs 4404；二进制 lossy）
- ① 不存在的 pty：rust `persistent_pty.rs:276-278` 升级前 HTTP 404；ref（persistent-pty.ts:126,152-158）一律升级后 `CloseEvent(4404, "terminal unavailable")`——WS 客户端错误路径完全不同。
- ② 非 framed 二进制输入：rust `persistent_pty.rs:482-484` `from_utf8_lossy`（U+FFFD 污染）；ref `persistent-pty.ts:203-216` 原样字节直传。G-10 覆盖的是 pty.rs 输出方向，本条是输入方向，独立。

### G-64 ❌ 插件 Context API 面残留（G-24 API 面追加）
- host.js:624-1056 Context 域清单缺：`provider.list/get/transform/reload`、`reference.list/transform/reload`（ref adapter.ts:348-352,448-452）；`experimental.terminal.read`（:334-337）；`integration.list/get/connect.key/oauth.{connect,status,complete,cancel}` 与 `connection.{active,resolve,status}`（:360-370,427-430，rust 仅 method.update）。依赖这些域的插件运行时 method_not_found。G-24 原文只记 hook 面，本条为 API 面。

### G-65 ❌ aisdk hook 注册面在、生产派发点为零（G-2 残留）
- 注册/桥接在（hooks.rs:916-919、plugin_apply.rs:894-909、host.js:827-830），但 `dispatch_aisdk_sdk/language` 生产 0 调用点（仅测试）。ref `aisdk.ts:346-354` 在 language model 构建时派发，且 `runLanguage` 可**替换** language model（变换点非纯观察）。rust 把 copilot lane 选择内联硬编码（copilot.rs:179 注释自认），插件无法介入。G-2 原文"3 个 hook 域已桥接"未覆盖此点。

### G-66 ❌ composer 选区高亮渲染缺失（模型层已备）
- rust 模型在（oc-tui-core/text_edit.rs:276,372-383 anchor/range API），渲染消费缺（composer/render.rs 无 selection 绘制）；upstream `EditBufferRenderable.ts:67-68,143-144,279` 有 `selectionBg/selectionFg`。`input.select.*` 键位有绑定但屏幕无反馈。（IME 半条已撤销：upstream 亦无 composition 渲染，终端模拟器自理。）

### G-67 ❌ 剪贴板读臂错误全折叠 None（G-39 读臂扩展）
- rust `clipboard.rs:388-399`：failed/timed-out/limit-exceeded 统一 `_ => None`，粘贴静默无操作。ref `clipboard.ts:30-35` 抛错，上层 `prompt/index.tsx:373-388` toast。超限粘贴（大图）用户无任何提示。

### G-68 ❌ mini fragment 调和与 image commits 缺失
- fragment reconciler（message part 重投影/restore append|covered|conflict，ref `mini/stream-v2.fragment.ts:1-83`）：rust mini 全目录 grep 0 命中；transport.rs:128-137 仅 delta 累加。
- mini 图片 commit + 去重（ref `stream-v2.image.ts:1-20`）：rust mini/ grep "image" 0 命中。
- （subagent tracker 已移植并撤销：transport.rs:455-620 对标 stream-v2.subagent.ts。）

### G-69 ❌ Diff base 对话框/记忆/懒缓存缺失（turn 模式已实现）
- rust `fetch_diff`（app.rs:6463-6551）每次 fresh 调 `vcs().base`，无记忆 Map/懒缓存；全库无 `vcs.branch.list` 消费——**无任何入口可选 base 分支**（错误文案 "Choose a base branch" 存在但无选择器）。ref `diff-viewer.tsx:110-124,280`（bases 记忆 + 懒缓存 + "Remembered until TUI exits"）。

### G-70 ✅ snapshot store 与 project id 不做 realpath（已修：store id 改为 canonicalize(worktree) 哈希；commit 022c5ef）
- rust `snapshot.rs:230-233` `hash_fast(worktree)` 无 canonicalize，且 Resolver 的 "canonical" 本身是词法的（project.rs:1252-1254、:964 自认）。ref `snapshot.ts:93-96` 先 `fs.realPath`（project.ts:338 同）。经 symlink 进入的仓库 → 不同 store，快照历史分裂。

### G-71 ❌ prettier/oxfmt 探测走项目 node_modules/.bin（同文件自相矛盾）
- rust `formatter.rs:585-598`（`npm_bin` 向上找 node_modules/.bin）供 prettier/oxfmt；同文件 Biome 分支（:708-718）反而正确走 `npm_which`。ref `builtins.ts:81,95` `Npm.which`（全局 cache + 缺失自动安装，npm.ts:400-435）。项目声明依赖但未安装（或 pnpm/bun 布局）时 rust 报 disabled、ref 可用。

### G-72 ❌ MCP stdio 缺 16MiB 帧上限与 stderr 尾巴诊断
- rust `mcp/client.rs` 无帧上限（rmcp 传输）；spawn 侧 `environment/stdio.rs:70-107` stderr **inherited** 不 pipe。ref `mcp/stdio.ts:15,121`（MAX_FRAME_BYTES=16MiB fail-fast）+ `:56,91,146-184`（stderr 滚动保留进崩溃诊断）。失控 server 可撑爆内存；崩溃只报 exit code 无线索。

## 🟢 低

### G-73 ❌ heredoc 剥离不容忍闭合标记尾随空白
- rust `patch_util.rs:105-108` `strip_suffix` 后仅剥一个 `\n`；`"EOF "`/`"EOF\n"` → 整个 heredoc 剥离失败 → "first line must be '*** Begin Patch'"。ref `util/patch.ts:381` 正则尾部 `\2\s*$` 容忍。（create_dir_all 半条已撤销：ref Bun.write 同样建父目录。）

### G-74 ❌ progress 事件缺口 + webfetch permission metadata 缺 timeout
- shell `context.progress({shellID})`（ref shell.ts:217）、websearch `progress({provider})`（websearch.ts:63）、codemode `progress({toolCalls})`（codemode/tool.ts:88）均零移植；webfetch permission metadata 仅 `{url,format}`（webfetch.rs:198-201 vs ref `metadata: input` 含 timeout）。

### G-75 ❌ session export/import CLI 行为偏差
- export 取消：rust 退出 0 + println 到 stdout（session.rs:245-247,156-158）；ref 130 + clack UI 全走 stderr（export.ts:30,53-66）——管道重定向时污染导出内容。
- import：rust 任意 JSON 直通；ref 经 SessionTransfer Schema 校验规范化（import.ts:25-26）。

### G-76 ❌ persistentPty.update payload 放宽
- rust `persistent_pty.rs:175-186`：size 可缺省（静默跳过 resize 仍 200）、非法值回退 80/24；ref schema `size` 必填 PositiveInt，缺失 → 400（persistent-pty.ts:36-39）。oc-schema 无 UpdateInput 类型。

### G-77 ❌ location.get 的 project.directory 被 canonical 覆盖
- rust `location.rs:40-44` 两字段同源 canonical；ref `project.ts:358-362` directory=实际检出目录、canonical=realpath（:43-44 注释明言语义不同，:135 依赖二者不等分支）。

### G-78 ❌ session.log 非法 sessionID 400 文案
- rust `sse.rs:493` message 为原始值回显；ref schema ParseError 形如 "Expected sessionID to be…"；rust 自身 after/follow 参数（sse.rs:504,518）已用该格式，sessionID 处不一致。一行修复。

## 归类与收编（不另立项）

- **POST /api/plugin/install（rust 独有路由）**→ G-17 rust-extra 家族（misc.rs:25 注释明言服务 G-29 安装流；ref protocol 仅 list/check/update）。
- **function 云分享服务端整包缺失** → 纯重复 G-17（:82 明列）。
- **oc-schema 缺 filesystem 非事件类型 + rpc.ts + v1/legacy** → G-32 子集（lib.rs:80-82 自认，已裁决有意）。
- **OTLP 遥测** → 纯重复 G-38（otel feature 非 default）。
- **OSC52 100KB 上限 + 空写直通** → 并入 G-39 备注：上限为防御性（终端普遍拒绝大 payload）且带显式错误；空写是对 v2 行为的有意 parity 修正（clipboard.rs:257-267 注释）。
- **codemode 沙箱架构（自研受限解释器 → Bun guest 真执行，无 Bun 即不可用）** → 收编为文档化决策（体例同 G-28/G-32）：codemode.rs:21-52 论证 + plugins.rs:241-255 接线；原仅记录于 PORT_AUDIT_2026-10-31.md M25。
- **评测/实验 lane（evaluation/system-one/opencode-zen/typesafe-ai）** → 归类 G-7/G-17；zen/typesafe-ai 唯一暴露面就是 experimental.evaluation（providers/opencode-zen.ts:1-25、typesafe-ai.ts:1-19），无常规 chat 导出。
- **AI SDK 公共 API 面（AIClient/promise/generation/tool builder）** → 归类 G-17 有意省略：ref 全仓无外部消费者（grep 0 导入），插件 tool 面已有等价（host.js:353、registrations.rs:95,574）。
- **非 Linux 音频走系统播放器 spawn** → 记录性差异：Linux（本构建目标）已进程内 miniaudio（sound/audio.rs:1-9，SOUND-CONV lane），非 Linux 为显式降级残留。
- **classify_provider_failure cause 恒 None** → 记录性差异：rust `ProviderFailure` 输入结构本无 cause 字段（provider_error.rs:317-329），错误链由 `#[source]` 上层承载；ref 中 cause 也不参与分类判定。
- **storybook/simulation** → 纯重复 G-8/G-17。

## 勘误（第五轮经核验撤销的初报缺口，勿再列入待修）

1. **read 工具丢 realpath 越界防护** — 初报对照错面：ref 工具边界同样是词法的（file-access.ts:67-95）；realpath 双栅栏属 HTTP FileSystem.read 服务，rust 侧已完整对齐（oc-server/handlers/fs.rs:119-141 `resolve_contained`：词法 contains → canonicalize → 二次 contains）。仅 AGENTS.md 发现走查用词法路径（tool/read.rs:855-868 vs ref realpath）属外观级差异。
2. **step.failed finish 强制映射 ContentFilter** — ref schema 将 finish 约束为单字面量 `["content-filter"]`（session-event.ts:379），updater `?? "error"`；rust StepFailFinish 同样只接受 content-filter（session_event.rs:383-412）——完全等价。
3. **engine.prompt 绕过 SessionPrompt.prepare** — seam 已在生产接线（engine/mod.rs:1497-1505 无条件）；drain 侧 backstop 覆盖裸 admit 路径（drain.rs:508-514 + prompt_prepare.rs PreparedRing 防双发），可观测面与 ref 等价。
4. **glob 搜索根≠location 显示路径基准不同** — 两侧 produce 相同绝对路径（glob.rs:151-154 `root.join(rel)` = glob.ts:131 `path.resolve(location.directory, rel)`），无模型可见差异。
5. **git.ts 服务未统一移植** — 唯一消费方 repository-cache 已完整移植（reference/repository_cache.rs:235-287：clone/fetch/checkout -B/reset --hard + daily 节流 + self-healing），无行为缺失。
6. **client permission 命名空间重组** — wire 逐路径一致（permission.rs:19-30 ↔ protocol/groups/permission.ts:23-117），仅内部函数命名不同。
7. **AIClient SDK 公共面 / cause 链 / IME / mini subagent tracker / diff turn 模式** — 见上文归类与撤销说明。

## 复审判定汇总（第五轮）
初报 35 项：CONFIRMED 新立 G-50..G-78（29 项，其中 G-63/G-64/G-68/G-69/G-75 各含 2 子项）；归类既有 G 条目 8 项（G-7/G-17 ×3/G-32 ×2/G-38/G-39）；RETRACTED 12 项（见勘误）；另订正 G-17 一处断言、勘误 #4 半边（→ G-56）。

---

# 第六轮复核（2026-10-31，第五轮同日：对文档现存未关闭条目的现状验证）

> 2 通道只读取证。目的：发现被后续提交悄然修复的项（收口标 ✅）、确认仍在的项、修正过时表述与路径。G-50..G-78 刚经第五轮双通道取证，不在本轮范围。

## 已收口（复核确认修复）

- **G-4 ✅（插件注册自定义 worktree strategy 已实现）**：guest `ctx.worktree.transform`（host.js:898-941）→ dispatch（supervisor.rs:356-369）→ `merge_worktree_strategies`（registrations.rs:494,643）→ `GuestWorktreeStrategy` 入注册表（plugin_apply.rs:685-695 → plugins.rs:2134-2151 → boot.rs:439-440）。测试 g24_domains.rs:96-98、g24_plugin_domains.rs:133。残留：注册表仍 boot 一次性（见 G-24 热重载子项）。project.rs:1103-1105 的"不支持"注释已失实（`add_strategy` 在 :1133 被调用）。
- **G-6 ✅（Shell 输出查看器 1s 轮询已实现）**：app.rs:8427-8437 对活跃 ShellOutput viewer 每秒重发 `LoopRequest::ShellView`；fetch 本体 app.rs:5635-5701。代码内 "G-6 residual" 注释已过时，可清理。
- **G-8 ✅（https/socks5 代理已支持）**：proxy.rs:10-14 模块头；socks5/socks5h 经 `tokio-socks`（socks5_connect :432-438），https 走 TLS-to-proxy（:318-320,:354），仅 socks4/4a 拒绝（:184-191，范围声明）；socks5 真桩 WS 端到端测试（:684）。proxy.rs:616 "G-8 residual" 注释已过时。
- **G-33 的"config 键无人消费"半边 ✅**：shell.rs:225 `with_output_limits` + engine/mod.rs:1053-1054 已消费。**G-48 热重载半边仍在**（见下）。

## 部分清偿（条目修正）

- **G-1 ⚠️（残留清单缩小）**：`mcp/client.rs` 已切 Environment stdio seam（client.rs:103 `use crate::environment::stdio::{...}`，:1059-1065,1152-1158，注释明言 workspace driver 扩展点）——从残留清单移除。仍直连 std::fs/portable_pty 的 8 个 consumer：read.rs（:179,388-401,713,764）、write/edit（经 file_mutation.rs:115,127-128,253-279）、tool/shell.rs（:227）、glob.rs（:124）、grep.rs（:427）、patch.rs（:401,417,426,444,452）、oc-core/pty.rs（:439 portable_pty）+ oc-server/handlers/pty.rs。
- **G-24 ⚠️（worktree guest-op 子项已清偿）**：vcs.transform/vcs.reload 仍缺（supervisor.rs:437 仅 `vcs.get|vcs.base`）；websearch `editor.default.set` 仍 no-op stub（host.js:871-874）；worktree 注册表热重载仍缺（`reload_after_update` plugins.rs:547-589 只做 entry swap，不重建 registry）。
- **G-31 ⚠️（VCS 根不限量已修）**：`scan_budget`（fs.rs:346-348）对 `is_vcs_root && !home_rooted → usize::MAX`，对齐 ref search.ts:72。仍在：每请求 spawn_blocking 全量 walk（:285-288）、无内存索引/10s 刷新（:330-332 自认 OUT OF SCOPE）、非 VCS 根仍 100k（:333）、`is_vcs_root` 仍为 `.git exists` 嗅探（:339-341，不认 hg/父仓子目录；fs.rs:260 `_services` 未用 `services.project.vcs`）。

## 确认仍在（不变）

- **G-34 ⚠️**：home_location 仍 write-only（set 唯一调用 command_palette.rs:3360；`home_location()` 唯一读者是测试 :3952；home 路由仍走 `footer_location()` home.rs:418、session.rs:1230,1318）。
- **G-48 ❌**：tool_output 仍 boot 一次性（Limits::from_entries 仅 boot.rs:301-302；`configure` 唯一调用 engine/mod.rs:503；config reload 路径无重赋值）。
- **G-41 ✅**：代理 userinfo 已保留并消费——CONNECT 发 Basic `Proxy-Authorization`、SOCKS5 用 username/password（lane2, 0f932c2）。
- **G-42 ✅**：WSS_PROXY/WS_PROXY 已优先咨询，再回落 HTTPS/HTTP(S)_PROXY/ALL_PROXY（lane2, 17947d9）。
- **G-43 ✅**：loopback（127.0.0.1/localhost/::1）已恒直连（lane2, 2d889a9）。
- **G-44 ✅**：NO_PROXY 已改 ref 语义——bare host 精确相等、`.`/`*` 前缀后缀匹配、空白+逗号分隔（lane2, df29fd3）。
- **G-45 ❌**：acp_drift.py:58 VENDORED 仍只覆盖主包，agent-client-protocol-schema 未守卫。
- **G-46 ❌**：dialog.plugins.error 仍 surfaceless（command_palette.rs:3121-3129 `Dispatch::Handled` 无操作）。
- **G-47 ❌**：IDE backoff 仍不复位（editor_ide.rs:222-234 attempt 只增不减）；空字符串 env 仍阻断 fallback（:36-39 `or()` 吞掉合法备选）。
- **G-49 ℹ️**：4 子项仍准确（spinner 受 mini/runtime.rs:38-39 TICK=100ms 约束；stdio.rs:11,:27 vs :104 措辞与实现不符；project.rs:1103-1105 过时注释——本条与 G-4 修复同源，可随清理收口；proxy.rs:367-369,:318 webpki 一致性声明在 G-26 后仍失效）。
- **G-28 ℹ️ / G-32 ℹ️**：决策现状未变（ui.rs:1-15 有界 slot bridge；oc-schema lib.rs:80-82 TODO 自认）。

## 路径勘误

- 文档多处 `oc-ai/src/...` 应为 `crates/oc-ai/src/...`（本轮引用已按现行路径取证；proxy.rs 内部行号基本未漂移）。

## 第六轮判定汇总
复核 15 项：✅ 收口 3（G-4、G-6、G-8）+ 1 半边（G-33 消费）；⚠️ 部分清偿 3（G-1 缩小、G-24、G-31）；❌/⚠️ 不变 8（G-34、G-41..G-48 各项、G-49）；决策项 2 现状不变（G-28/G-32）。

过时注释清理：已完成（2026-10-31 第六轮同日）——app.rs:8427/store.rs:7089/:14996（G-6 residual，共 3 处 + app.rs:7340）、proxy.rs 模块头/`:318`/`:616`/`:661`（G-8 residual + 失效的 webpki 一致性声明，改为指向 G-26 决策）、project.rs:1103-1105（插件 strategy 已支持，指向 GuestWorktreeStrategy）、environment/stdio.rs 模块头与 `StopChild` doc（SIGTERM 措辞改为与 `start_kill` 实现一致的"immediate SIGKILL + bounded reap wait"）。均为注释级修改，无行为变化；oc-ai 的 fmt 差异为 llm.rs 既有问题，与本次无关。
