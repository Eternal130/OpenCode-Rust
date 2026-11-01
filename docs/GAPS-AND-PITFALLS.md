# 移植缺口与坑（Gaps & Pitfalls）

> **2026-11-01 第九轮（七域全量复审）**：7 通道对 ref 全功能域（core 会话/工具/AI/server/TUI/CLI/schema-plugin-pty-syntax）逐行对读后，对初报缺口逐项源码复核：确认 12 项新立项（G-93..G-104）、撤销 3 项初报（见文末第九轮勘误）、5 项归类既有条目。详见文末第九轮。

> **2026-11-01 第七轮 burndown（gap-burndown-4）**：G-50..G-78 全部 + G-41..G-48 残留 + G-31/G-34/G-24 残留 + G-1 consumer 迁移（read/write/edit/glob/grep/patch/shell/pty 全走 Environment seam）经 lanes 1-9 串行落地并合并 main（merge commits 8f5c7a2..c289c2b：lane1 G-50/51/52/53/54/70；lane2 G-41..G-44；lane3 G-55/57/58/59/60/63/76/77/78；lane4 G-56/48/34/33；lane5 G-64/65+G-24 残留+G-46；lane6 G-66/67/68/69/47；lane7 G-61/62/71/72/73/74/75/45；lane8 G-31；lane9a/9b G-1）。本轮（lane10）核验并钉定 G-49a spinner 40ms 帧数学（帧索引按流逝时间派生，repaint 维持 pump tick——文档化决策），并完成文档终扫。现存未关闭项均为**文档化决策**：G-28 有界 TSX shim、G-32 v1/legacy schema 有意不移植、G-12/G-13/G-16/G-17/G-38/G-39/G-40 ℹ️ 维持不变；少量 deferred minors 见 .superpowers/sdd/ 各 lane report（如 G-68 mini restore 保留 seam、G-24 guest vcs provider info/base 闭包留 guest 侧、G-36 demo 剧本 6/13 bounded、G-9 nested scope 简化）。

> **2026-10-31 第六轮复核**：对文档现存未关闭条目逐项现状验证——G-4/G-6/G-8 已悄然修复收口；G-1 残留缩小（mcp/stdio 已迁）、G-24/G-31 部分清偿；G-34/G-41..G-48 确认仍在。详见文末第六轮。

> **2026-10-31 第五轮全量对比审查**：9 通道全量对读 + 4 通道缺口复核，新立 G-50..G-78（含 5 项🔴：G-50 snapshot 保真、G-51 persistent-pty Origin、G-52 debug config 脱敏、G-53 skill 输出、G-54 run 表单退出码），订正 G-17 断言与勘误 #4 半边，撤销 12 项初报。详见文末第五轮。

> **2026-10-31 收口轮（gap-burndown-3）**：G-18..G-27、G-29..G-31、G-33..G-37 全部落地并合并 main（单 lane 串行、worktree 隔离、dev-gate、合并门测试，日志在 .test-logs/merge-*）。残留清单与 G-1/G-6/G-8 残留项同轮清偿（MCP stdio 经 Environment seam、Shell 查看 1s 轮询、https/socks5 WS 代理）。G-26 双栈信任统一（oc-http crate，platform-verifier + webpki 降级）。G-28 维持有界化设计（声明为插件生态 breaking change）、G-32 v1/legacy schema 维持有意不移植（纯 v2 面无影响）——两项裁决为文档化决策。仍开放的工程残留：G-1 tools/handlers 全量 consumer 迁移（read/write/edit/shell/glob/grep/patch、pty）、G-24 有界外项（guest 侧 worktree host-op 调用、vcs.transform/reload、websearch editor.default.set 持久化、worktree 注册表热重载）。（G-31 内存索引已由 lane8 收口。）
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
- 2026-11-01 consumer 迁移 part 1（lane9a）：read/write/edit/glob/grep/patch 六个文件工具全部改走 location 绑定的 Files seam（`ToolServices.files` + `FileMutation::with_files`，engine 侧与 MCP stdio seam 同款 local 回退）。新增 Files trait 默认方法 `remove_file`（保持 `std::fs::remove_file` 的 NotFound/EISDIR 失败形状，patch delete/move 腿用）与 `write_file`（不隐式建父目录，patch move-write 腿用），以及 `FilesError -> io::Error` 映射（NotFound→NotFound、WrongKind(dir)→EISDIR 21、Failed→原始 io 字符串），本地驱动行为逐字节不变（LocalFiles 即原 std::fs 路径）。read 的分块读上限（256KiB chunk / 20MiB media）恒严于 Files 契约的 64MiB 读上限，实际生效的始终是工具自身上限。
- 2026-11-01 consumer 迁移 part 2（lane9b）——G-1 consumer 侧收口 ✅：
  - tool/shell.rs：spawn 前的工作目录存在性/类型校验从 `std::fs::metadata` 改走 `ToolServices.files` 的 follow 语义探针 `type_following`（location 绑定 Environment Files seam；本地驱动行为与原 follow-symlink metadata 等价——符号链接目录通过、悬空链接报 NotFound→"does not exist"，错误文案不变；`Files::stat` 本身是 lstat 语义，不可直接用）；cwd 默认值仍为 location 目录（services.directory，与 ref 一致），exec-defaults 协议（44/45/46）未动。
  - oc-core/pty.rs：portable_pty spawn 机制（openpty/reader/writer/spawn_command/clone_killer）原样移入 `environment/pty.rs` 的 `PtySpawner` seam（`LocalPtySpawner` 本地驱动 + `FailingPtySpawner` memory 驱动对位），Driver 增加 `pty` capability，`Environment.pty(location)` 按位置解析；`PtyService.create` 走默认（本地）spawner，新增 `create_with` 供按位置传入；会话机（缓冲/订阅/事件/保留上限）不变，killer/resizer 经中性 trait（`PtyChildKiller`/`PtyMaster`）。
  - oc-server/handlers/pty.rs：`POST /api/pty` 创建经 `state.environment.pty(location)` 解析（与 Files seam 同款 bind 失败→local 回退）；list/get/update/remove/write/attach 会话操作不经 spawn，保持原样。
- 剩余已知非迁移（文档化决策）：grep/glob 的 rg walker 子进程 spawn 仍在 host 侧（仅搜索路径探针经 Files seam；walker 属 rg 内部实现，迁移它超出 Files 协议面）；persistent_pty 仍是独立移植路径（ref 侧同样独立于 `Pty.Service` spawn 面），其迁移待非 local 驱动注册后按需进行。
- 已知有意残留（workspace.rs 模块文档列明）：ref 的共享 provision/连接池/wake wrapper 仅在非 local provider 注册后才有意义。

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

### G-8 ✅（可落地部分已落地；三包维持有意不移植）
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

### G-28 ℹ️（文档化决策：插件 TSX 渲染为有界 shim，兼容性降级）
- rust 有 JSX 面（host.js:1857-1900 @opentui/solid shim 的 createElement/setProp/insert + supervisor.rs:1051-1111 ui.slot.register/router.register），但渲染树跨 IPC 时拍平为 `{title, rows, clickable}` 有界结构（supervisor.rs:964-966）。无任意组件树/markdown code-block renderer 注册面。
- 行式插件（sidebar 类）可跑；富 TSX 布局插件无法忠实渲染。ℹ️ 属有界化设计而非疏漏，但需作为插件生态 breaking change 显式声明。

### G-29 ✅(bounded) TUI 插件管理页：安装/更新/检查流无操作
- rust command_palette.rs:2905-2909 有插件列表+active 翻转+错误详情；但 `plugins.install | dialog.plugins.install | update | check` 显式 `Dispatch::Handled` 无操作（:3033-3047，注释 "registered-but-surfaceless"）。ref system/plugins.tsx 358 行完整管理 UI。CLI `opencode plugin add` 可绕过。

### G-30 ✅(bounded) 快照用 git2 且无 opencode.gitconfig 等价配置
- ref git.ts:23,28（`autocrlf=false` 的 opencode.gitconfig）、:535（`-c core.quotepath=false`）。rust snapshot.rs:248-278 用 libgit2，grep gitconfig/autocrlf/quotepath 零命中——还会不受控地继承宿主机 `~/.gitconfig`。
- 后果：CRLF 检出工作区快照 diff 整文件误报；非 ASCII 路径引用语义不一致。低成本修复：git2 `Repository::config` set_str。

### G-31 ✅ 文件模糊搜索索引 + VCS 根判定（lane8 收口：resolver 判根 + 内存索引 10s 刷新）
- ~~rust oc-server/handlers/fs.rs:254-330：每请求 spawn_blocking 全量 walk；SCAN_ENTRY_BUDGET=100_000 统一上限~~ lane8 已收口：`is_vcs_root` 改用 project resolver（`services.project.vcs` + canonical 根相等，hg 仓库生效；仓库子目录 location 保持 100k 保守语义，与旧 `.git` 嗅探一致）；`/api/fs/find` 改读进程级 `SearchIndex`（AppState 字段）——按 canonical root 键控、10s 后台刷新（spawn_blocking walk，失败保留上次好索引）、首访同步构建（build-on-first-miss）、并发请求 single-flight；非 VCS 根 100k 预算在**索引构建时**施加。排名/打分语义不变（仅候选来源改为索引）。
- 残留偏差（文档化）：VCS 判根沿用「检出根才算」的保守语义（ref 的 `location.vcs` 对仓库内任意目录都为真值）；失效策略为 root 键控 + 新鲜度窗口重建（ref 亦无显式 config 失效钩子），未做显式 config-reload 钩子。fix1 加固：single-flight claim 由 Drop guard 持有（取消/panic 不泄漏）+ 首建等待有界（30s）；slot 自淘汰（根目录消失或 5 分钟无访问即驱逐、停刷，rebuild-on-miss 保证安全）；后台刷新每 tick 重读最新请求参数（budget/exclusions 不再冻结于 spawn 时）。另注：常驻 10s 后台刷新任务是本 lane brief 指定项（ref 实为按需 refresh-on-access），额外稳态开销已知。

### G-32 ℹ️（文档化决策：v1/legacy schema 有意不移植，纯 v2 面无影响）
- oc-schema/src/lib.rs:80-82 TODO 仍在：legacy-event、v1 session 事件、models-dev refresh、rpc.ts、pty-ticket、durable-event-manifest。仅影响旧数据互操作/回放，纯 v2 面无影响。

## 🟢 低

### G-33 ✅ shell 输出截断上限 config 失联（热重载半边由 G-48 lane4 收口，全项 ✅）
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

- **G-34 ✅（lane4 收口）**：原 PARTIAL 半边已接——`set_home_location` 写通共享 cell，`HttpPromptSink` 按 home_location 解析 home create-directory（inherit → 当前会话目录，ref app.tsx:718-737），home 状态行显示继承目录。
- **G-30 ℹ️（补充）**：repo-local autocrlf/quotepath 钉定当前行为惰性——快照管道全程 raw blob/OID-map/treebuilder/手写 diff，从不咨询 git config；parity 为偶得（raw passthrough 即无 CRLF 误报）。钉定仅防未来 libgit2 操作。ref 另钉 7 个 key（longpaths/symlinks/fsmonitor/untrackedCache/manyFiles/index.version/threads，ref git.ts:23-33）未移植，当前行为中性。
- **G-31 ⚠️（补充）**：`is_vcs_root` 弱于 ref `location.vcs`——仓库子目录 location 与 hg 仓库在 rust 侧仍 100k 截断（保守方向）；handler 手里已有 `services.project.vcs`（fs.rs:260 绑定 `_services`）却用文件系统嗅探。
- **G-19 ⚠️（补充）**：`validate_config`（mantle_messages.rs:199-221）零调用——ref 的 bearer-without-key fail-fast（ProviderConfigurationError）永不触发，rust 静默 SigV4 回退；`auth/profile/region/credentials` settings 键在 oc-core ModelConnection seam（baseURL/apiKey/headers，facade.rs:123-136）无处解析（承自 converse lane 的既有模式）。
- **G-24 ✅（lane5 清偿全部 bounded 外项）**：`vcs.transform`/`vcs.reload` 已接（supervisor 派发 + 注册表合并）、websearch `editor.default.set` 已持久化（引擎 KV）、worktree 注册表热重载已接（`reload_after_update` 后重跑 `plugin_worktree_registry` 播种，`AppState.worktree_strategies` 可热换）。guest 侧 worktree host-op 此前已清偿。

## 新登记缺口（G-41..G-49）

### 🟡 中
- **G-41 ✅ 代理 URL 凭据被丢弃**：已修复——userinfo 解析保留（percent-decode），CONNECT 发 Basic `Proxy-Authorization`，SOCKS5 username/password 认证。
- **G-42 ✅ WSS_PROXY/WS_PROXY 未咨询**：已修复——WS 族优先于标准族（websocket-constructor.ts:43-47 语义）。
- **G-43 ✅ 无 loopback 目标绕过**：已修复——127.0.0.1/localhost/::1 恒直连。
- **G-44 ✅ NO_PROXY bare-host 语义偏差**：已修复——bare host 精确相等、`.`/`*` 前缀后缀匹配、空白+逗号分隔。

### 🟢 低
- **G-45 ✅（lane7）**：acp_drift.py 已按 crate 参数化（每 crate 钉 sha256、独立 pristine env 覆盖 ACP_SCHEMA_PRISTINE_DIR），agent-client-protocol-schema 1.11.0 与主包同样走 upstream+LOCAL_CHANGES.patch 端到端校验，guard 对两包跑通。
- **G-46 ✅（lane5）**：dialog.plugins.error 打开失败插件的 error 详情（ErrorDetails 对话框，esc 回插件列表；无失败行为无操作）。
- **G-47 ✅（lane6）**：连接成功即复位 backoff attempt（editor.ts:223 open 监听）；空字符串 CLAUDE_CODE_SSE_PORT 现按 ref falsy `||` 落到 OPENCODE_EDITOR_SSE_PORT。
- **G-48 ✅（lane4）**：reload bridge 在失效 location 前重导 `Limits::from_entries` 并 `Engine::reload_tool_output_limits`（boot.rs ConfigReloadBridge），assemble 改读 `ToolOutput::current_limits` 活值——改 `tool_output.*` 免重启生效。
- **G-49 ℹ️ 记录性/文档债（spinner 子项收口为文档化决策，G-49a/lane10）**：spinner 帧进阶 ✅ 经核验/钉定——帧索引按渲染时刻的流逝时间取（`composer_spinner_cell(elapsed_ms)`：`elapsed_ms/40 % frames.len()`，knight.rs），ref 的 40ms 帧数学在任意重绘频率下精确成立（含 100ms pump tick 采样：index 恒 = floor(elapsed/40)），单测 `frame_progression_follows_the_40ms_cadence_math` 钉死； repaint 帧率维持 pump tick（100ms）为**文档化决策**——运行循环无 composer 区域级重绘源，全局 40ms 唤醒会扰动泵（共享 PTY harness 以 >80ms 静默窗切快照、已登记的 vt_temporal 时序测试钉定 100ms 节奏、空转字节量护栏），故按 brief 采用时间派生索引这一最小忠实修复；session.environment 每 session 单发（ref 效果重触发，PUT 幂等无害）；stdio.rs 模块头宣称 SIGTERM→SIGKILL 升级但实现为即杀（沿袭 rmcp 行为，措辞与 ref 2s 优雅等待不符）；project.rs:1103-1105 注释"插件 strategy 不支持"已过时（add_strategy 就在其下）；proxy.rs:367 注释"webpki 与 0.13 栈一致"在 G-26 后已失效（0.13 现走 platform-verifier；ws 链路留 webpki 为有意）。

## 复审判定汇总
G-18 ✅ / G-19 ✅(死校验残留) / G-20 ✅ / G-21 ✅ / G-22 ✅ / G-23 ✅ / G-24 ✅(bounded) / G-25 ✅ / G-26 ✅ / G-27 ✅(F1=新 G-45) / G-29 ✅(error 面残留) / G-30 ✅(惰性钉定) / G-31 ✅(lane8：resolver 判根 + 10s 内存索引；子目录/失效策略偏差文档化) / G-33 ✅(热重载半边由 G-48 lane4 收口) / G-34 ✅(lane4) / G-35 ✅ / G-36 ✅ / G-37 ✅ / G-6 ✅ / G-8 ✅(G-41..G-44 已由 lane2 收口) / G-41 ✅ / G-42 ✅ / G-43 ✅ / G-44 ✅ / G-1 ✅(consumer 迁移仍开放)

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

### G-55 ✅ shell create 缺省 cwd 继承服务器进程 cwd（已修：handler 侧 cwd None/空回退 location.directory，对齐 shell.ts:265；commit lane3）
- rust `shell.rs:562` `unwrap_or_default()`，`create_invocation` 仅非空时 `current_dir`（:648-650）。工具路径安全（tool/shell.rs:165-168 显式覆写）；**生产命中点：HTTP `POST /api/shell`**（oc-server/handlers/shell.rs:40-52 直接透传可能为 None 的 body.cwd）。ref `shell.ts:265` `input.cwd ?? location.directory`。修复：handler 侧用 location 兜底。

### G-56 ✅ 凭据/wellknown 变更不驱动 config 重载与 location 失效（勘误 #4 半边修正；lane4 收口：boot 订阅 credential.switched/integration.updated → request_reload + location 失效，watch_loader 加 .with_wellknown，refresh 变更时发布 integration.updated 驱动周期刷新后重载）
- 刷新循环与 HostWellknownSource 接线属实（engine/mod.rs:570-596、:900-903）——勘误 #4 只对了这一半。缺的半边：①全仓 `request_reload` 生产调用点为零（仅 WatchedConfig 自身文件 watch 驱动，config.rs:655,671）；②credential/integration 事件不触发 location invalidate（ConfigReloadBridge 只挂文件 watch，boot.rs:113-140）——已 assemble 的 location 持旧 wellknown 快照直到其他原因失效；③boot 的 watch_loader 无 `.with_wellknown`（boot.rs:241-266）。
- ref `config.ts:306-322` 订阅 Credential.Switched / WellKnown.Updated → `reload()`；10 分钟刷新后亦 reload（:323-341）。与 G-48 同族不同面。

### G-57 ✅ 删除非 active 凭据多发 `credential.switched(credentialID:null)`（已修：store 发布门改为仅删 active 才发 Switched，对齐 credential.ts:314-319；handler 原本已正确）
- rust `integration/credential.rs:331-360`：非 active 分支返回 `Some((Some(integration), None))`，发布门只判 integration——非 active 删除也发 Switched。oc-server/handlers/credential.rs 同病。
- ref `credential.ts:268-269,314-319`：`{switched:false}` 只发 updated；仅删 active 才发 Switched。修复：发布门加 switched 判定。

### G-58 ✅ Session.create 项目解析仅目录身份（已修：插件 session.create 传 services.project 完整解析，对齐 session.ts:271）
- rust `store.rs:604-609` project 缺省回落 `hash("directory:…")`。HTTP/命令/transfer 主路径均显式传 `services.project`（完整 Resolver，engine/mod.rs:872-887）；**生产命中点：插件 `session.create`**（oc-server/plugins.rs:1300-1313 `CreateInput::default()` → project=None）。ref `session.ts:271` 无条件 `projects.resolve`（含 realpath + VCS 发现）。

### G-59 ✅ 工具输入校验为 JSON-Schema 子集（已修：扩展 pattern/oneOf/anyOf/allOf/$defs+$ref/minItems/maxItems/uniqueItems/additionalProperties；format 保持宽松；无新依赖）
- MCP schema 有注册并校验（mcp/registry.rs:1749 → tool/registry.rs:383 `validate_input`），真实缺口是子集语义：rust `registry.rs:88-232` 仅 type/enum/min/max/required/items 等，pattern/oneOf/$defs 引用等静默放行。ref `runtime.ts:107-113,251-258` 经 jsonSchema() 全量 draft-07/2020-12 转换校验。坏参数穿透到 leaf。

### G-60 ✅ list_mcp_resources 模型内容固定占位文本（已修：content 改为 stringify(catalog) 全 JSON）
- rust `tool/mcp_resource.rs:213` content 恒 `"Listed MCP resources"`；ref `runtime.ts:57,259-264` normalizeContent → stringify(catalog) 全文。模型在直接对话路径拿不到目录内容（output JSON 仍正确）。

### G-61 ✅（lane7）：CLI auth login 无 target 时走交互集成选择器
- TTY 无 target 时弹 dialoguer 选择器：loginChoices 排序（MCP → Popular[优先级表] → Services，name/id 决胜）+ connected ✓ 标记，选中后按 id 解析；非 TTY 保持 requireInteractive 报错。

### G-62 ✅（lane7）：CLI plugin check/update 覆盖 TUI(npm) 插件
- check 合并 Server+TUI 分区输出（ref inventory.ts `format`：每行 `name version (update available|current|check failed)`，hex 版本截 7 位），TUI 项走 oc-plugin-host `npm_check`（无 24h 缓存）；update 对 TUI 项走 npm update 安装流，任一 check/update 错误 exit 1。

### G-63 ✅ persistent-pty connect 协议分歧（已修：未知 pty 一律升级后 4404 关闭；非 framed 二进制输入原样字节直传 write_bytes）
- ① 不存在的 pty：rust `persistent_pty.rs:276-278` 升级前 HTTP 404；ref（persistent-pty.ts:126,152-158）一律升级后 `CloseEvent(4404, "terminal unavailable")`——WS 客户端错误路径完全不同。
- ② 非 framed 二进制输入：rust `persistent_pty.rs:482-484` `from_utf8_lossy`（U+FFFD 污染）；ref `persistent-pty.ts:203-216` 原样字节直传。G-10 覆盖的是 pty.rs 输出方向，本条是输入方向，独立。

### G-64 ✅ 插件 Context API 面残留（G-24 API 面追加）（lane5 收口）
- 已全部落地：`ctx.provider.list/get/transform/reload`（editor 表含 models 子编辑器；transform defs 经 `apply_plugin_providers` 折入 location overlay）、`ctx.reference.list/transform/reload`（`apply_plugin_references` 入 ReferenceService）、`ctx.experimental.terminal.read`（persistent-pty Frontend.read）、`ctx.integration.list/get/connect.key/oauth.{connect,status,complete,cancel}/command.*` 与 `connection.{active,resolve,status}`（oc-plugin-host ProviderPort/ReferencePort/TerminalPort/IntegrationPort + supervisor 异步派发 + oc-server Engine*Port 槽位模式）。guest 集成测试 g64_domains.rs。注：`ctx.vcs.transform` 的 vcs provider 行进注册表快照，但 guest vcs adapter 未接 project 解析（无既有宿主缝，见 lane5 report concerns）。

### G-65 ✅ aisdk hook 注册面在、生产派发点为零（G-2 残留）（lane5 收口）
- 生产派发点已接：`SessionModelRequest::prepare`（model_request.rs）在 language model 绑定请求处派发 `aisdk.sdk` → `aisdk.language`（aisdk.ts:346-354 的 runSDK → runLanguage 链；sdk 句柄流入 language 事件；返回的 language 句柄**替换**请求目标——变换点；language 事件的 options 并入 provider options，copilot lane 的 `endpoint` 覆盖由此流入，无 hook 时内联 lane 选择保持默认）。测试 model_request.rs aisdk_dispatch_tests。

### G-66 ✅（lane6）：composer 选区以反色（fg/bg 交换，EditBufferRenderable 无 selectionBg 默认支）渲染；live 路由 shift+方向键已映射（crossterm spec 层此前丢弃）
- rust 模型在（oc-tui-core/text_edit.rs:276,372-383 anchor/range API），渲染消费缺（composer/render.rs 无 selection 绘制）；upstream `EditBufferRenderable.ts:67-68,143-144,279` 有 `selectionBg/selectionFg`。`input.select.*` 键位有绑定但屏幕无反馈。（IME 半条已撤销：upstream 亦无 composition 渲染，终端模拟器自理。）

### G-67 ✅（lane6）：剪贴板 failed/timed-out/limit-exceeded 读按 clipboard.ts 抛错语义变为 Err，粘贴路径 toast.error 展示（消息逐字对齐 ref）
- rust `clipboard.rs:388-399`：failed/timed-out/limit-exceeded 统一 `_ => None`，粘贴静默无操作。ref `clipboard.ts:30-35` 抛错，上层 `prompt/index.tsx:373-388` toast。超限粘贴（大图）用户无任何提示。

### G-68 ✅（lane6，fix1 后仅 restore 为保留 seam）：mini/fragment.rs 移植 stream-v2.fragment.ts；project/delta 覆盖消费/end 已接入 transport 折叠 + 回放生产路径（replay_history→replay_message_commits：重放投影进同一 reconciler，图片 commit 随回放触发，重复回放不重绘）；mini/image.rs 移植 stream-v2.image.ts（kind user/tool、metadata+payload 身份去重先于 mime 过滤）。restore(append|covered|conflict) 为保留 seam：rust mini 为 mono 写直写终端回滚区、无本地行缓冲重放（ref 的 restore 消费点是 resize-replay 重纳本地行），无生产消费点
- fragment reconciler（message part 重投影/restore append|covered|conflict，ref `mini/stream-v2.fragment.ts:1-83`）：rust mini 全目录 grep 0 命中；transport.rs:128-137 仅 delta 累加。
- mini 图片 commit + 去重（ref `stream-v2.image.ts:1-20`）：rust mini/ grep "image" 0 命中。
- （subagent tracker 已移植并撤销：transport.rs:455-620 对标 stream-v2.subagent.ts。）

### G-69 ✅（lane6）：bases 记忆（TUI 退出前记住）+ 按目录懒缓存（vcs.base 一次，**任何**关闭路径失效——Store::close_diff_viewer 统一失效）+ DiffBase 选择器（vcs.branch.list，Diff source 对话框 Base 行）
- rust `fetch_diff`（app.rs:6463-6551）每次 fresh 调 `vcs().base`，无记忆 Map/懒缓存；全库无 `vcs.branch.list` 消费——**无任何入口可选 base 分支**（错误文案 "Choose a base branch" 存在但无选择器）。ref `diff-viewer.tsx:110-124,280`（bases 记忆 + 懒缓存 + "Remembered until TUI exits"）。

### G-70 ✅ snapshot store 与 project id 不做 realpath（已修：store id 改为 canonicalize(worktree) 哈希；commit 022c5ef）
- rust `snapshot.rs:230-233` `hash_fast(worktree)` 无 canonicalize，且 Resolver 的 "canonical" 本身是词法的（project.rs:1252-1254、:964 自认）。ref `snapshot.ts:93-96` 先 `fs.realPath`（project.ts:338 同）。经 symlink 进入的仓库 → 不同 store，快照历史分裂。

### G-71 ✅（lane7）：prettier/oxfmt 探测改走 npm_which
- PackageJsonDependency 分支改用与 Biome 相同的 `npm_which`（全局 cache 世代 + 按需自动安装），依赖探测仍以 findUp package.json 为门；项目 node_modules/.bin 链 `npm_bin` 已删除。

### G-72 ✅（lane7）：MCP stdio 16MiB 帧上限 + stderr 尾巴诊断
- rmcp codec 拥有分帧，故上限在受控 seam 实施：交给传输的 reader 包 `FrameCapReader`（按行计数、超限 fail-fast 报 "MCP stdio frame exceeded 16 MiB"，读缓冲回滚遵守 AsyncRead 错误契约）；LocalStdioSpawner stderr 改 pipe + 后台 drain 进 8KiB 滚动尾巴，本地 server connect 失败附带尾巴文本（短 grace 等待 drain）。

## 🟢 低

### G-73 ✅（lane7）：heredoc 剥离容忍闭合标记尾随空白
- strip_heredoc 闭合标记后先 trim_end 再剥标记与前置换行，`"EOF "`/`"EOF\n"`/`"EOF \t\n\n"` 均正常剥离。（create_dir_all 半条已撤销：ref Bun.write 同样建父目录。）

### G-74 ✅（lane7）：progress 事件 + webfetch permission metadata
- 共享 `ProgressFn` 通道（runner 级 session.tool.progress 发布器，同 subagent）：shell spawn 后 `{shellID}`、websearch 结果边界 `{provider}`（rust query seam 内部定 provider，故在结果边界发）、codemode 叶子 `{toolCalls}`（runtime seam 对逐调用更新不可见，程序落定后发一次完整清单）；webfetch permission metadata 为完整输入 `{url,format,timeout}`。

### G-75 ✅（lane7）：session export/import CLI 行为对齐
- export：选择器 clack 风格状态输出全走 stderr（stdout 保留给导出内容），取消 exit 130、无会话仍 exit 0；import：先按 SessionTransfer 形状（`{info: Session.Info, messages: []}`）校验，坏载荷报清晰错误拒绝。

### G-76 ✅ persistentPty.update payload 放宽（已修：oc-schema 增 UpdateInput；size 必填、非法 400，无 80/24 回退）
- rust `persistent_pty.rs:175-186`：size 可缺省（静默跳过 resize 仍 200）、非法值回退 80/24；ref schema `size` 必填 PositiveInt，缺失 → 400（persistent-pty.ts:36-39）。oc-schema 无 UpdateInput 类型。

### G-77 ✅ location.get 的 project.directory 被 canonical 覆盖（已修：directory=检出目录、canonical=main worktree realpath）
- rust `location.rs:40-44` 两字段同源 canonical；ref `project.ts:358-362` directory=实际检出目录、canonical=realpath（:43-44 注释明言语义不同，:135 依赖二者不等分支）。

### G-78 ✅ session.log 非法 sessionID 400 文案（已修：schema ParseError 格式 `Expected sessionID to be …`）
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
- **G-33 的"config 键无人消费"半边 ✅**：shell.rs:225 `with_output_limits` + engine assemble 已消费。**G-48 热重载半边 ✅（lane4）**。

## 部分清偿（条目修正）

- **G-1 ⚠️（残留清单缩小）**：`mcp/client.rs` 已切 Environment stdio seam（client.rs:103 `use crate::environment::stdio::{...}`，:1059-1065,1152-1158，注释明言 workspace driver 扩展点）——从残留清单移除。仍直连 std::fs/portable_pty 的 8 个 consumer：read.rs（:179,388-401,713,764）、write/edit（经 file_mutation.rs:115,127-128,253-279）、tool/shell.rs（:227）、glob.rs（:124）、grep.rs（:427）、patch.rs（:401,417,426,444,452）、oc-core/pty.rs（:439 portable_pty）+ oc-server/handlers/pty.rs。
- **G-24 ✅（lane5 清偿 bounded 外项）**：vcs.transform/vcs.reload 已接（supervisor 派发 + 注册表合并；reload 走 VcsPort——EngineVcsPort 实时计算，reload 为契约面）；websearch `editor.default.set` 已持久化（host.js → WebsearchPort::set_default → `WebSearch.select` 引擎 KV，与 ref 的 auth/config 存储等位）；worktree 注册表热重载已接（`reload_after_update` 返回是否有代际移动，plugin/update handler 重跑 `plugin_worktree_registry` 播种；`AppState.worktree_strategies` 移入 `Arc<RwLock<…>>` 可热换）。guest vcs provider 的 info/base 闭包仍留 guest 侧（无宿主缝，见 lane5 report）。
- **G-31 ✅（lane8 收口）**：~~VCS 根不限量已修~~ 进一步：`is_vcs_root` 改用 project resolver（hg 生效、仓库子目录保守=100k，与旧根嗅探语义一致）；`/api/fs/find` 改读进程级 per-canonical-root `SearchIndex`（10s 后台刷新、build-on-first-miss、single-flight；非 VCS 根 100k 在索引构建时施加）。偏差文档化见上文 G-31 条目。

## 确认仍在（不变）

- **G-34 ✅（lane4）**：home_location 已接消费端——`set_home_location` 写通共享 cell，`HttpPromptSink` 提交时按其解析 home create-directory（inherit → 当前会话目录），home 状态行显示继承目录。
- **G-48 ✅（lane4）**：config reload 路径重赋值（bridge 重导 Limits + `reload_tool_output_limits`，assemble 读 `current_limits` 活值）。
- **G-41 ✅**：代理 userinfo 已保留并消费——CONNECT 发 Basic `Proxy-Authorization`、SOCKS5 用 username/password（lane2, 0f932c2）。
- **G-42 ✅**：WSS_PROXY/WS_PROXY 已优先咨询，再回落 HTTPS/HTTP(S)_PROXY/ALL_PROXY（lane2, 17947d9）。
- **G-43 ✅**：loopback（127.0.0.1/localhost/::1）已恒直连（lane2, 2d889a9）。
- **G-44 ✅**：NO_PROXY 已改 ref 语义——bare host 精确相等、`.`/`*` 前缀后缀匹配、空白+逗号分隔（lane2, df29fd3）。
- **G-45 ✅（lane7）**：guard 已按 crate 参数化，schema 包同样守卫（LOCAL_CHANGES.patch 已提交）。
- **G-46 ✅（lane5 收口）**：dialog.plugins.error 打开失败插件的 error 详情（plugins.tsx:224-228 的 "view error"）——ErrorDetails 对话框（可滚动、esc 回插件列表；无失败行为无操作）。
- **G-47 ✅（lane6 收口）**：backoff 复位 + falsy env fallback 均已修（editor_ide.rs ConnectOutcome.opened → attempt=0；env_editor_port filter 空串）。
- **G-49 ℹ️**：4 子项仍准确（spinner 受 mini/runtime.rs:38-39 TICK=100ms 约束；stdio.rs:11,:27 vs :104 措辞与实现不符；project.rs:1103-1105 过时注释——本条与 G-4 修复同源，可随清理收口；proxy.rs:367-369,:318 webpki 一致性声明在 G-26 后仍失效）。
- **G-28 ℹ️ / G-32 ℹ️**：决策现状未变（ui.rs:1-15 有界 slot bridge；oc-schema lib.rs:80-82 TODO 自认）。

## 路径勘误

- 文档多处 `oc-ai/src/...` 应为 `crates/oc-ai/src/...`（本轮引用已按现行路径取证；proxy.rs 内部行号基本未漂移）。

## 第六轮判定汇总
复核 15 项：✅ 收口 3（G-4、G-6、G-8）+ 1 半边（G-33 消费）；⚠️ 部分清偿 3（G-1 缩小、G-24、G-31）；❌/⚠️ 不变 8（G-34、G-41..G-48 各项、G-49）；决策项 2 现状不变（G-28/G-32）。

---

# 第八轮（2026-11-01，独立九域审查 + 双通道逐项核验）

> 来源：`CODE_REVIEW_PARITY_2026-11-01.md`（9 域并行逐行审查，明细 `/tmp/opencode/review/L1..L9-*.md`）。本轮对该报告的 21 项缺口指控做 2 通道源码级核验：**确认 14 项**（下列 G-79..G-92，续编号）、**驳回 4 项**（见文末勘误，勿再立项）、**已追踪 2 项**（browser 工具→G-8 有意决策；语法高亮语言裁剪→G-13/R-01b）、**1 项并入 G-32 备注**（schema TODO 未明确子项）。

## G-79 ❌（🔴 数据完整性）Session.remove 不完整 → 孤儿子会话
- Rust：`oc-core/src/session/store.rs:557-562` 仅发布 `session.deleted`；`oc-server/handlers/session.rs:505-516` handler 直调 store.remove；projector（`session/projector.rs:148-152`）只删 session_v2 行。`execution.await_idle` 存在（handlers/session.rs:1542 他处使用）但 remove 路径未调用。
- Ref：`packages/core/src/session.ts:367-377` interrupt→awaitIdle→transport.close→**递归删子会话**→environments.clear→bus.remove。
- 影响：删除运行中会话不先中断/等待；子会话（parent_id 指向已删父）成孤儿行；environments 不清理。bus map 清理半边已有反应式等价物（`bus/routing.rs:125,168` 对 session.deleted）。
- 修法提示：remove 前置 interrupt+awaitIdle；递归收集 parent_id 后代一并删除；补 environments 清理。

## G-80 ❌（🔴 指令面泄漏）`<available_skills>` 缺 permission deny 过滤
- Rust：`session/runner/context.rs:678-704` filter 仅看 `description.is_some() && auto_invoke != Some(false)`，无 `Permission.evaluate("skill", id)`（对比同文件 :1587 `mcp_guidance_source` 显式收 permissions——照抄该模式即可）。
- Ref：`packages/core/src/skill.ts:33-34`（`available()` 过滤 `effect !== "deny"`）。
- 影响：被 deny 的技能仍进系统提示，模型调用后才被工具层拒绝，浪费一轮工具调用。

## G-81 ❌ Vercel AI Gateway gatewayProtocol 缺失
- Ref：`packages/ai/src/protocols/utils/gateway-protocol.ts:29-90`（`providerOptions.gateway` 注入请求体 + finish 事件合并 `provider_metadata.gateway` usage/cost）+ `providers/vercel-ai-gateway.ts`。
- Rust：grep 全 oc-ai/oc-core 仅见 cache_policy.rs:50 / customization.rs:303 的缓存与 auth 处理；`variant.rs:861 vercel_gateway` 只做 thinking 变体折叠，无 body 注入、无 finish 网关 metadata 合并。
- 影响：gateway 路由选项（caching/order/models 等）不下发；finish 事件丢失网关侧 usage/cost（计费可观测缺口）。

## G-82 ⚠️ tool.progress 不回写 LlmEventPublisher（死代码机制）
- Rust：`engine/mod.rs:2273-2288` ProgressFn 直发 bus；`session/runner/publisher.rs:867-881` 的 `progress()`（记录 + 发布）**全仓零生产调用点**；而 `publisher.rs:962-977` failureSnapshot 与 subagent aborted `(sessionID: …)` 后缀都依赖 `tool.progress` 已记录。
- Ref：`session/runner/publish-llm-event.ts:562-572`（progress 先记录再发布）、:336,352-355。
- 影响：live `session.tool.failed` 恒缺进度元数据；subagent 中止错误永远不带 `(sessionID: …)` 后缀（仅 stale 清算路径有）。易修：engine 的 ProgressFn 改走（或同时调用）publisher.progress。注意 G-74 只覆盖"发事件"半边，本条是"记录"半边。

## G-83 ⚠️ which-key 面板：11 条绑定为死表
- Rust：`oc-tui-core/src/keymap/table.rs:805-815` 注册 11 条 which-key 绑定；唯一"消费者"是 `oc-tui/components/command_palette.rs:3187-3197` 显式吞键无操作（注释自认 "registered-but-surfaceless"）。
- Ref 背景：TUI 自身也无消费者——面板由 `@opentui/keymap` 库 `createDefaultOpenTuiKeymap` 提供（`context/keymap.tsx:13,66`）；Rust 自研 keymap 未复刻该 addon。
- 影响：ctrl+alt+k 等 11 组键被静默吞掉、无帮助面板。决策项：要么补面板，要么删绑定并在文档声明取舍（与 G-29 同类）。

## G-84 ❌ alibaba-responses 专用适配器缺失
- Ref：`packages/ai/src/protocols/alibaba-responses.ts`（96 行：`enable_thinking`/`previous_response_id`/`conversation` 注入、named tool_choice→`allowed_tools` 变换、`web_search`/`web_extractor`/`code_interpreter` 托管工具 onDone 还原）。
- Rust：`provider_catalog/variant.rs:1089` 将 `alibaba/responses` 归并到通用 `openai_responses`；`enable_thinking` 仅 chat 路径（openai_chat.rs:2992）、`allowed_tools` 仅 provider options 直读（open_responses.rs:1114-1120）、`web_extractor` 全仓零命中。
- 影响：阿里 Model Studio responses 面上 thinking 开关、会话续传、命名工具强制、托管工具输出还原均不可用。（同类存疑：meta-responses / bedrock-mantle responses 也归并到通用路径，见 L5 报告。）

## G-85 ⚠️ read 工具 alternatePath 缺 NFC 归一
- Rust：`tool/read.rs:790-798` canonical 仅做 NBSP/弯引号替换，缺 `name.normalize("NFC")` 前置。
- Ref：`tool/plugin/read.ts:139-143`（先 NFC 再替换）。
- 影响：macOS NFD 分解形式文件名与用户 NFC 输入无法互配 → alternatePath 静默失配报 not found。修法：canonical 前加 unic-normal NFC。

## G-86 ⚠️ opencode.models 工具 own_provider 未接线
- Rust：`tool/opencode_tools.rs:518` 注册时 `Mutex::new(None)`；:442 执行时读取；全仓无生产回填点（仅 tests 预填）。Ref：`tool/plugin/opencode.ts:134-197` 按 session.model.providerID 排 own-provider-first。
- 影响："Your own provider comes first" 恒不生效（低危，输出排序）。

## G-87 ⚠️（并入 G-32 备注）schema TODO 未明确子项
- G-32 已裁决 v1/legacy、durable-event-manifest、pty-ticket 等；**未明确**的三个子项：event-manifest 索引、snapshot store types、`Worktree.adopt` 投影（`oc-schema/src/lib.rs:80-83`、`misc.rs:121-123`）。其中 Worktree.adopt 影响会话/项目归因（worktree.remove 后会话归属），建议单独评估是否升级。

## G-88 ⚠️（安全语义）MCP OAuth RFC8707 resource 拒绝分支弱化
- Ref：`mcp/oauth.ts:168-171` `checkResourceAllowed` 不通过即 `throw "Protected resource ${resource} does not cover ${identity}"`。
- Rust：`mcp/oauth.rs:255-281` 仅 URL 推导；全 mcp 目录 grep `checkResource|does not cover` 零命中。
- 影响：服务器返回 resource 与配置 identity 不一致时不再拒绝——token 可能绑定到身份不匹配的 resource。建议补齐拒绝分支 + 测试。

## G-89 ⚠️ webfetch 超时语义分叉
- Rust：`tool/webfetch.rs:110-112` 仅 reqwest client timeout；:232-235 超时以 `Nested(error)` 落 cause，message 恒 "Unable to fetch {url}"；convert 阶段不在预算内。
- Ref：`webfetch.ts:146-148` `Effect.timeoutOrElse` 包整个流程，文案 `"Request timed out"`。
- 影响：模型可见的超时反馈不同，重试行为可能漂移。修法：外层 timeout 包全流程并输出 ref 文案。

## G-90 ⚠️ patch formatter 阶段静默容错
- Rust：`tool/patch.rs:490-493` read 失败 `continue`；:497-498 formatter sync 失败仅 `unwrap_or(current.text)`。Ref：`patch.ts:261,265-267` 分别 `fail("Failed to read/sync ${target}")`。
- 影响：部分失败时模型看到成功但 diff 缺格式化内容。

## G-91 ⚠️ session.http.response hook 的 request 回传为空
- Rust：`session/runner/model_request.rs:1128-1133` response 事件 `request` 硬编码空（url/method/headers/body）。Ref：`model-request.ts:340` `request: before.request` 完整回传。
- 影响：审计/重放类插件读不到触发请求。修法：把 before 的 request 快照透传。

## G-92 ⚠️ util 基础设施分裂（glob brace/extglob + MIME 三处漂移 + Windows spawn/npm）
- glob：`oc-util/src/glob.rs:7-8` 自认 "Braces and extglobs are not supported"（ref minimatch 支持）；:10-11 Glob.scan "deferred"——ignore 配置含 `{foo,bar}` 等模式时语义分叉。
- MIME：`FSUtil.mimeType` 仅落在 `oc-server/assets.rs:237-239`，另 `tool/read.rs:138`、`oc-tui/composer/attachment.rs:23` 各有私有表——三处漂移风险，应收敛为单一 util。
- Windows：cross-spawn-spawner（.cmd shim/PATHEXT/shebang）与 npm.ts（npm_execpath 探测）未整体移植——非 Windows 目标裁决可挂靠 G-8/G-17，但插件安装/updater 在 Windows 是已知分歧面。

## 第八轮勘误（经核验驳回，勿再立项）

- ~~迁移 journal id 不一致~~：`migrations.rs:205`=`20260410174513_workspace-name`、`:287`=`20260511173437_session-metadata`，与 ref 文件名逐一相同（`_workspace` 是另一条 20260225215848 迁移，双方同名存在）。初报对照错位。
- ~~models-dev 默认不刷新~~：`EngineOptions.catalog_fetch=false` 是 hermetic 默认，生产 boot **无条件置 true**（`boot.rs:309`），`OPENCODE_DISABLE_MODELS_FETCH` truthy→off 与 ref `fetch: !truthy(...)` 对齐（boot.rs:329）。
- ~~event-logger 缺失~~：已移植（`boot.rs:160-190` `event_logger_line`/`attach_event_logger`，:376-382 生产安装，boot.rs:1487-1545 测试）。
- ~~account 表持久化缺失~~：三表全移植（`migrations.rs:140,162-183` + `schema.rs:472-498` + bootstrap 建表）。
- ~~MCP stdio BUN_BE_BUN 仅测试断言~~：生产已注入（`mcp/client.rs:1067-1079` `stdio_overlay`，:1158 生产 spawn 路径调用）。

## 第八轮判定汇总
核验 21 项：确认立项 14（G-79..G-92，其中 🔴 数据/安全级 2：G-79、G-80；安全语义 1：G-88）；驳回 5（含第八轮勘误 5 条）；已追踪 2（G-8、G-13）。优先级建议：G-79 > G-80 > G-82 ≈ G-88 > G-81 > G-84 > G-91 > G-85 > G-89 ≈ G-90 > G-83（决策项）> G-86 > G-87 > G-92。

过时注释清理：已完成（2026-10-31 第六轮同日）——app.rs:8427/store.rs:7089/:14996（G-6 residual，共 3 处 + app.rs:7340）、proxy.rs 模块头/`:318`/`:616`/`:661`（G-8 residual + 失效的 webpki 一致性声明，改为指向 G-26 决策）、project.rs:1103-1105（插件 strategy 已支持，指向 GuestWorktreeStrategy）、environment/stdio.rs 模块头与 `StopChild` doc（SIGTERM 措辞改为与 `start_kill` 实现一致的"immediate SIGKILL + bounded reap wait"）。均为注释级修改，无行为变化；oc-ai 的 fmt 差异为 llm.rs 既有问题，与本次无关。

---

# 第八轮 burndown 收口（2026-11-01，gap-burndown-5）

> G-79..G-92 全部 14 项经 lanes A-G 串行落地并 fast-forward 合并 main（abac5af / 8a53326 / 981180a / 45c7358 / 7cfcd4e / fddac63 / 72a6422）。全程 worktree 隔离（`.worktrees/`，合并后 target 与 worktree 均已回收）、dev-gate 单入口、nextest 未过滤验证、日志落盘 `.test-logs/*-lane[ABCDEFG].log`。各 lane 报告含字面 `Summary:` 行与 `git log -1` 落地证明。

- **G-79 ✅（laneA，abac5af）**：新增 `oc_core::session::remove_tree`（session/mod.rs）——interrupt → 有界 awaitIdle（10s timeout，删除不受 drain 卡死挟持，理由代码内注明）→ 按 parent_id 递归删后代（子先父后）→ session_env.clear → store.remove/session.deleted；DELETE handler 改调 remove_tree。transport.close 无独立调用：drain 中断 unwind 已关传输通道（drain.rs close_transport），代码内注释说明。测试：递归删除、先中断后删（wedged drain 证序）、HTTP wire 父/子/孙 404。
- **G-80 ✅（laneA，abac5af）**：`skill_guidance_source` 接收 Ruleset，经 `permission::evaluate("skill", id)` deny 过滤（镜像 mcp_guidance_source 模式 / ref skill.ts:33-34）。测试：denied skill 不入 `<available_skills>`。
- **G-88 ✅（laneB，8a53326）**：MCP OAuth RFC8707 resource 覆盖校验——`check_resource_allowed`（origin 相等 + 请求路径长度 ≥ 配置路径 + 尾斜杠填充前缀比较，`/api` 不覆盖 `/api123`）+ `validate_resource_url`（缺失 resource → 配置 identity；不覆盖 → `Err("Protected resource {resource} does not cover {identity}")`），接入 discover。测试覆盖通过/拒绝两侧全矩阵。
- **G-81 ✅（laneC，981180a）**：`protocols/utils/gateway_protocol.rs`（GatewayProtocol 包装：三 API prepare、`{...upstream, gateway}` body 注入、跨帧 `merge_json_records` 合并 gateway usage/cost 到 finish 事件含 on_halt）+ `protocols/vercel_gateway.rs`（三条路由工厂 + x-session-affinity）+ facade `ProtocolFamily::VercelGateway` 接线与快照断言。
- **G-84 ✅（laneC，981180a）**：`protocols/alibaba_responses.rs` 专有协议（enable_thinking / previous_response_id / conversation / named tool_choice→allowed_tools / native.alibaba 三托管工具 onDone 还原含 web_extractor item 恢复）+ `ProtocolFamily::AlibabaResponses` 路由（variant.ts:571 侧映射核对无需求改）。**laneC 顺带发现 meta-responses 同族缺口** → laneE 收口。
- **G-82 ✅（laneD，45c7358）**：`publisher::ProgressLog` 共享记录表；engine 五处 ProgressFn 点先记录后发布（publish-llm-event.ts:562-572 对位）；failureSnapshot 与 subagent aborted `(sessionID: …)` 后缀消费记录，settle 时清理。测试：tool.failed 携带 progress。
- **G-85 ✅（laneD，45c7358）**：read canonical() 前置 NFC 归一（unicode-normalization 0.1.26 最小引入）再 NBSP/弯引号替换。测试：NFD 落盘名 + NFC 请求互配。
- **G-86 ✅（laneD，45c7358）**：`ModelsExecute` 携 SessionStore，执行时按 session.model.providerID 解析 own-provider-first（opencode.ts:134-142），engine 接线 tool_services.db。
- **G-89 ✅（laneD，45c7358）**：webfetch 全流程（fetch+collect+convert）入 tokio timeout 预算，超时文案 "Request timed out"；reqwest client 超时改 +5s 兜底；非超时失败保留 url 文案。测试：stalled server 与 refused port 两分支。
- **G-90 ✅（laneD，45c7358）**：patch formatter resync 读失败 → `fail("Failed to read {target}")`、sync 失败 → `fail("Failed to sync {target}")`；已删目标过滤出 resync 集合（patch.ts:256-258，全量跑撞出的回归同轮修复）。测试经 read-failing Files 包装。
- **G-91 ✅（laneD，45c7358）**：response 事件经 per-interceptor-pair stash 回放 before-request 快照（url/method/headers/body）。测试：request hook 改写 URL 后 response hook 观察到整形后请求。
- **G-83 ✅（laneF，fddac63，推荐方案=补面板）**：有界 which-key 面板——前缀键（如 `<leader>` ctrl+x）触发底部 overlay 列出后续键+表内标签（排序、12 行封顶，无 timeout/滚动面）；chord 完成/esc/未绑定续键关闭；`which-key.toggle`（ctrl+alt+k）真实派发根列表；其余 10 个 which-key.* 家族键维持 consumed no-op（有界端口不带面板 chrome，注释说明）。PTY 级验证：真实终端按 ctrl+x 断言标签渲染与关闭。
- **G-87 ✅（laneG，72a6422）**：三子项逐一裁决——**Worktree.adopt 已实现**（`oc_schema::misc::worktree_adopt`：candidate 检查含 workspace 排除与显式 adopted 列表、ref 路径归一（`.`/`..` 折叠 + Windows 盘符大小写）、subpath 投影 + 6 单测）；event-manifest 索引**本已移植**（oc-server/event_manifest.rs + bus registry + drift 守卫，TODO 注明）；snapshot store types**文档化不移植**（ref 6 行 branded-ID 模块无 wire 消费者，实体 store 已在 oc-core/src/snapshot/）。
- **G-92 ✅（laneG，72a6422，代码+文档双收口）**：三处私有 MIME 表收敛为 `oc_util::mime`（依赖方向核验：oc-util 零新依赖）；read.rs `.xml` 修正为 `application/xml`（原值对 npm mime-types 参照即漂移）；attachment 保留 ref 受限 allowlist 语义不变。glob brace/extglob 与 Windows cross-spawn/npm_execpath 以注释级"已知有界分歧"钉定（G-8/G-17 族）。
- **meta-responses ✅（laneE，7cfcd4e，laneC 新发现同族项）**：`protocols/meta_responses.rs`（770 行忠实移植：native.meta web_search/image_generation 校验、signed-ID replay 变换（provider 执行的 image_generation 结果按 itemId 回放 raw item handle）、托管工具表、text-end annotations 合入 providerMetadata、completedItems 双键去重）+ `ProtocolFamily::MetaResponses` 路由（variant.rs 映射核对无需改）。

## 第八轮 burndown 判定
G-79..G-92 全部 ✅（G-83 采用补面板方案；G-87 三子项实现/已移植/文档化各一；G-92 代码收敛+分歧注释）。现存未关闭项维持既有文档化决策：G-28（有界 TSX shim）、G-32（v1/legacy 不移植，G-87 子项已并注）、G-12/G-13/G-16/G-17/G-38/G-39/G-40 ℹ️。

---

# 第九轮（2026-11-01，七域全量复审 + 逐项源码复核）

> 来源：7 通道并行逐行审查（core 会话/消息/Agent、内置工具、AI 层、server/API、TUI、CLI/client/SDK、schema/theme/util/plugin/pty/syntax），明细即本轮报告。对全部初报缺口逐项重新取证后：**确认 12 项**（G-93..G-104，续编号）、**撤销 3 项初报**（勘误见文末）、**归类既有条目 5 项**（不另立项）。风险排序：🔴 高 1 / 🟡 中 7 / 🟢 低 4。

## 🔴 高

### G-93 ❌ `$EDITOR` 往返丢失全部 mention 附件
- Rust：`oc-tui/src/app.rs:8165-8172` 编辑器返回后仅 `composer.set_text(&normalized)`；而 `components/composer/mod.rs:759-773` 的 `set_text` 无条件 `draft = PromptDraft::default()` 并清空 files/agents/skills 的全部 undo/redo 栈——`@文件/@agent/@skill` 附件静默全丢。
- Ref：`packages/tui/src/prompt/index.tsx`（`$EDITOR` 返回分支）调 `realignPromptInputMentions`（`prompt/mention.ts:20-140`，DP 最大匹配按最小代价把附件重挂到编辑后文本）+ `restoreExtmarksFromPrompt`。
- 修法提示：移植 realign DP，或在编辑器往返前快照附件、返回后按 mention 文本仍在文中重挂。

## 🟡 中

### G-94 ❌ `POST /api/session/:id/permission` 丢弃 payload 的 agent/metadata/source
- Rust：`oc-server/src/handlers/permission.rs:189-194` 三字段硬编码 `None`（仅 resources 透传 :191）。
- Ref：`protocol/groups/permission.ts:63` 全量透传写入 `Permission.Request`。
- 影响：依赖 request.metadata（权限弹窗上下文）/agent 归属的客户端行为缺失。

### G-95 ❌ `POST /api/session/:id/form` 丢弃 payload 的 metadata
- Rust：`oc-server/src/handlers/form.rs:115` `metadata: None` 恒置空。Ref `FormCreatePayload` 含 metadata 并透传。

### G-96 ❌ `GET /api/session/:id/context` 未按 compaction 截断
- Rust：`oc-server/src/handlers/session.rs:1226-1245` 返回**全部**消息（`ListOrder::Asc` 全量）。
- Ref：`session.ts:567` 语义为"最近一次 compaction 之后的活动上下文"（latestCompaction 之后的窗口）。
- 影响：长会话经该端点取上下文的客户端拿到被压缩掉的历史。

### G-97 ⚠️ permission.saved list/remove 引入 ref 没有的 location 装配副作用
- Rust：`oc-server/src/handlers/permission.rs:58-66`（saved_list）/remove 路径先 `resolve_location` + `services()`——冷启动触发 SSE 可见的 boot 级联、目录无效返回 404（注释自认是"v2 location middleware"，但 ref 中这两端点注册于组中间件**之前**、不经 location 解析）。
- Ref：`groups/permission.ts:37,49` 无 location 副作用、无该 404 路径。

### G-98 ⚠️ webfetch 非法 timeout 的校验时机后移至权限 assert 之后
- Rust：`oc-core/src/tool/webfetch.rs:208`（permission.assert）→ `:236` 才 `resolve_timeout`；`timeout<=0/>120` 的输入在解码期不拒绝（注释 AUD-19② 自认可知），用户会先看到授权弹窗、批准后才收到 "Invalid arguments"。
- Ref：`webfetch.ts:29` schema 解码期即拒绝，不触发权限确认。
- 注：与 G-89（超时预算/文案，已修）不同面，本条是**校验顺序**。修法：`resolve_timeout` 提到 assert 之前。

### G-99 ⚠️ `upgrade --method curl` 语义偏差（执行自有 release 下载而非官方安装脚本）
- Rust：`oc-cli/src/services/updater.rs:328` `"github" | "curl" => upgrade_from_release(...)`，`:454` 显式 `let _ = via_installer_semantics;`——curl 与 rust-extra 的 github 方法共用"下载本移植自身 release tarball"路径。
- Ref：curl 方法执行官方安装脚本（`curl -fsSL https://opencode.ai/install`）。
- 影响：用户显式 `--method curl` 得到不同发行流。修法：要么实现 installer 脚本路径，要么将 curl 从方法表移除/改名，避免静默语义替换。

### G-100 ⚠️ `LLM.generateObject` 的 jsonSchema 模式输出校验为手写子集
- Rust：`oc-ai/src/llm.rs` `validate_json_schema` 仅覆盖 type/enum/required/properties/items（+递归）；`oneOf/anyOf/allOf/$ref/minLength/pattern/minimum/additionalProperties` 等静默放行。
- Ref：Effect JsonSchema 完整解码，畸形输出报 `InvalidProviderOutputError`。
- 关联：G-59 已修**工具输入**校验子集（tool/registry），本条是 llm.rs **输出**校验器，独立半边。动态 schema（MCP/插件清单）调用方防错弱于 ref。

### G-101 ⚠️ workspace file-changes 对话框降级为纯 yes/no 确认
- Rust：`oc-tui/src/components/command_palette.rs:778-783` 仅固定文案 "This worktree has file changes." + yes/no。
- Ref：`dialog-workspace-file-changes.tsx:29-60` 渲染逐文件 `A/D/M` 状态与 `+N/−N` 增删计数（宽度自适应、8 行滚动窗）——强删 worktree/forceRequired 场景用户看不到将丢弃哪些改动。

## 🟢 低

### G-102 ⚠️ 滚轮加速模型缺失（scroll.acceleration/speed 配置无行为效果）
- Rust：`oc-tui/src/store.rs:6624` 注释自认 wheel 流无加速模型；`dialog_config.rs:303-308` 的 `scroll.acceleration` 行可切换但无消费点（默认步长 3 已对齐，默认路径一致）。
- Ref：`tui/src/util/scroll.ts:20-28`（MacOSScrollAccel / speed 默认 3）被 12 个表面消费。

### G-103 ⚠️ `app.exit` 默认键位缺 `<leader>q`
- Rust：`oc-tui-core/src/keymap/table.rs:511` 仅 `ctrl+c,ctrl+d`（注释 :510 说明裁决：`<leader>q` 单归 session.queued_prompts）。
- Ref：`config/keybind.ts:48` `ctrl+c,ctrl+d,<leader>q`（与 :131 queued_prompts 同键并存，靠注册顺序仲裁）。行为面不完全一致——习惯 `<leader>q` 退出的用户得到队列面板而非退出。

### G-104 ⚠️ `stats --limit` 缺 `>=1` 下界
- Rust：`oc-cli/src/cli.rs:348` 裸 `u32`，`--limit 0` 可解析（渲染层随后报上限提示）。
- Ref：`commands.ts:332-335` Schema 要求 `>=1`。修法：`value_parser!(u32).range(1..)` 一行。

## 归类与收编（不另立项）

- **experimental AI 模块（evaluation/system-one）缺失** → 已有裁决：G-7（有意不移植，ref 主流程不消费）/G-17。
- **markdown 代码围栏无语法 injection、csharp/kotlin/swift feature 门控** → 纯重复 G-13。
- **插件进程外 bun 沙箱（超时/崩溃重启/热重载）与 TSX 有界 shim** → G-28（文档化决策）。
- **CodeMode execute 走插件宿主 guest 运行时** → 第五轮已收编为文档化决策（codemode.rs:21-52）。
- **`POST /api/plugin/install` rust 独有端点** → G-17 rust-extra 家族（第五轮归类项）。
- **Session.remove 10s 有界等待 / restart 恢复 / model-transport** → G-79 ✅ 已收口（remove_tree + 有界等待为文档化决策；jobs.rs `prepare_resume`/`DEFAULT_MAX_RESUME_ATTEMPTS`/`RESUME_EXHAUSTED_MESSAGE` 均已实现；model_request.rs:888,1075-1186 有 http_interceptors + WS handshake 对等面）。

## 第九轮勘误（经复核撤销的初报，勿再立项）

1. **restart 恢复服务未移植** — 初报未找到等价物，实际 `oc-core/src/jobs.rs:636-726` 已有 `prepare_resume`/`RESUME_EXHAUSTED_MESSAGE`/`DEFAULT_MAX_RESUME_ATTEMPTS`（对齐 restart.ts:14-24,67-84）。
2. **model-transport 拦截层不完整** — `model_request.rs:888,915,1071-1186` 已有 `http_interceptors`（request/response 对，response 可回放 before-request 快照=G-91 修复产物）与 WS handshake 派发面。
3. **persistent-pty 回放分块 1MiB（与 G-10 的 64KiB 冲突）** — 复核 `oc-server/src/handlers/pty.rs:342` 确认按 64KiB 字符边界分块；G-10 记录正确，1MiB 说法是初报误读（1MiB 为 read 分块上限，非 WS 回放帧）。

## 第九轮判定汇总
初报 20 项：确认立项 12（G-93..G-104，🔴 1：G-93；🟡 7：G-94..G-101；🟢 4：G-102..G-104）；归类既有 5（G-7/G-13/G-17/G-28 + G-79 关联组）；撤销 3（见勘误）。优先级建议：G-93 > G-98 ≈ G-94 > G-96 ≈ G-99 > G-95 ≈ G-97 ≈ G-100 ≈ G-101 > G-102 ≈ G-103 ≈ G-104。

---

# 第十轮 burndown 收口（2026-11-02，gap-burndown-6）

> G-93..G-104 全部 12 项经 lanes 1-4 串行落地并 fast-forward 合并 main（55032d4 / 4e5a55c / 56d5584 / 8d2aef7）。全程 worktree 隔离（`.worktrees/`，合并后已回收）、dev-gate 单入口、nextest 未过滤验证、日志落盘 `.test-logs/*-lane[1-4].log`。各 lane 报告含字面 `Summary:` 行与 `git log -1` 落地证明。

- **G-93 ✅（lane1）**：`oc-tui/src/components/composer/mention_realign.rs` 全量移植 ref mention.ts realign DP（group-by-text、longest-first、保护区间、memoized 最小代价最大匹配）；`Composer::set_text_realigned` 保留幸存 @file/@agent/@skill 附件；$EDITOR 返回分支改调之。偏差文档化：char-offset DP（非 display-width）、无文本 mention 丢弃。
- **G-94 ✅（lane2）**：permission create handler 透传 payload agent/metadata/source（AssertInput 字段本已有，handler 此前硬编码 None）。注：Permission.Request 双侧均无 agent 字段，agent 流入 AssertInput（wire 断言 metadata/source）。
- **G-95 ✅（lane2）**：form create 透传 payload metadata。
- **G-96 ✅（lane2）**：`GET /api/session/:id/context` 截断至最近一次**已完成** compaction 行之后的窗口（ref history.ts latestCompaction gte 语义；running/failed compaction 不扩窗）。
- **G-97 ✅（lane2）**：`GET/DELETE /api/permission/saved` 不再 resolve_location/装配 services（对齐 ref 前置中间件注册）；无效目录不再 404、不触发 boot 级联。
- **G-98 ✅（lane3）**：webfetch timeout（<=0 / >120）校验移至 permission.assert 之前（ref 解码期拒绝语义）；测试证明非法 timeout 不触发权限弹窗。
- **G-99 ✅（lane4）**：`upgrade --method curl` 走官方安装脚本路径——下载 `https://opencode.ai/v2/install` 后 `<bash> <installer> --version <v> --no-modify-path`（对齐 ref updater.ts，非 pipe-to-sh 初稿设想）；github 保持 release 下载；`OPENCODE_INSTALLER_URL` env 覆盖；网络/spawn 桩测试。
- **G-100 ✅（lane3）**：G-59 JSON Schema 校验器上移为共享 `oc-ai/src/json_schema.rs`（oc-core→oc-ai 依赖方向合规，registry.rs re-export 保持 API）；generateObject 输出校验补齐 oneOf/anyOf/allG、$defs+$ref、minLength/maxLength/pattern、minimum/maximum、minItems/maxItems/uniqueItems、additionalProperties、type 联合；零新依赖。
- **G-101 ✅（lane1）**：workspace file-changes 对话框渲染逐文件 A/D/M + 路径 + `+N/−N` 计数（8 行窗口 + 省略提示）；数据经 worktree worker 在 forceRequired 拒绝时拉 vcs.status（失败降级空表）。偏差：固定 8 行窗口（非滚轮 scrollbox）。
- **G-102 ✅（lane1）**：`oc-tui/src/scroll_accel.rs` 字节级移植 MacOSScrollAccel（3 样本窗、150ms streak、`1+0.8·(e^(v/3)−1)` 钳 6×）+ CustomSpeed + 配置序；`Store::wheel_step` 消费 live `scroll.acceleration`/`scroll.speed`。偏差：整行取整、transcript 保持分页步长映射。
- **G-103 ✅（lane1）**：`app.exit` 默认键位补 `<leader>q`（keybind.ts:48）；队列面板存活时（queuedPrompts 非空）app.exit 对 chord 续键让位——复刻 ref 注册序仲裁。
- **G-104 ✅（lane4）**：`stats --limit` 加 `value_parser!(u32).range(1..)`，0 在解析期拒绝。

## 第十轮判定
G-93..G-104 全部 ✅。现存未关闭项维持既有文档化决策：G-28（有界 TSX shim）、G-32（v1/legacy 不移植）、G-12/G-13/G-16/G-17/G-38/G-39/G-40 ℹ️。
