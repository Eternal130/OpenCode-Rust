# 移植缺口与坑（Gaps & Pitfalls）

> **2026-11-03 第十二轮（全量对比审查）**：22 通道逐文件全文对读（TS ~21 万行 / Rust ~40 万行生产代码）+ 3 通道缺口核验，对 50 项缺口声称逐项双侧源码取证：**新立项 46 项（G-165..G-210，含 🔴×6：Azure 未接线/Mcp.Protocol wire bug/Copilot anthropic 车道/第三方 anthropic 根 URL/shell 注释换行权限逃逸/transfer 脱敏弱）**、撤销 4 项初报 + 6 项子句修正（见文末第十二轮勘误）、5 项归类既有条目。总报告 `CODE_REVIEW_PARITY_2026-11-03.md`，域报告与核验记录 `/tmp/opencode/review/`。

> **2026-11-02 第十一轮 burndown 收口（gap-burndown-7）**：第十轮立项的 G-105..G-164 全部 60 项经 lanes 1-9 落地并合并 main（lane1 ce86cd7 / lane2 a3485a4 / lane3 ed23543 / lane4 b0a026f / lane5 56e8d36 / lane6 e8c3b7f / lane7 18363aa / lane8 34837c8 / lane9 本轮合并），逐条状态改标 ✅（G-163/G-164 维持 ℹ️ 登记项，G-161② 已修）。各 lane 有界偏离清单见文末第十一轮。

> **2026-11-02 第十轮（全量逐行复审）**：9 通道逐行对读 + 6 轮补审 + 4 通道缺口复核，对全部初报缺口逐项双侧源码取证后确认 60 项新立项（G-105..G-164）、驳回 13 项初报（见文末第十轮勘误）、若干 DUP 归并既有条目（G-6/G-17/G-32/G-49a 已追加备注）。总报告 `CODE_REVIEW_PARITY_2026-11-02.md`，明细 `/tmp/opencode/review/`。

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
- 2026-11-02 第十轮勘注：本条"忠实移植"仅覆盖静态注册表面——`@`-file.find 实时模糊搜索（异步半边）未移植（autocomplete.rs:23-24,483-484,819 注释自认），见 G-127；Terminals tab 仅注册 3 键位、行数据层空壳不可达（keymap/table.rs:837-843 注释自认）。

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
- rust-extra 超出 ref 表面的部分（`upgrade -m local-file|github`、completion、`--force`、run 的 TTY 权限提示、serve hostname 解析报错、`$PWD` 陈旧值守卫等）均为防御性收紧且有注释。run 的**非 TTY** 路径与 ref 一致（noninteractive.rs:289 auto-reject）。
- 2026-11-02 第十轮归并：tui feature-plugins/system/storybook 工作台（~1400 行开发期 fixture）同属本条整包不移植族；workerd 运行时目标与 ServerFetch.make（结构性不可移植，Rust 仅原生 boot_and_serve）收编为本族文档化范围裁剪（登记为 G-163 ℹ️）。

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
- 2026-11-02 第十轮复核备注：上述五个 schema 子项经逐项核验**全部在本条明文范围内**（G-87 已裁决过未明确子项，维持 DUP）；另 run 的 v1 事件输出兼容层（ref run/v1.ts：pendingStep/v1InvalidOutput/permissionReject 静默等）同属本裁决范围——消费 v1 事件流的脚本不受支持。

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
- **G-49 ℹ️ 记录性/文档债（spinner 子项收口为文档化决策，G-49a/lane10）**：spinner 帧进阶 ✅ 经核验/钉定——帧索引按渲染时刻的流逝时间取（`composer_spinner_cell(elapsed_ms)`：`elapsed_ms/40 % frames.len()`，knight.rs），ref 的 40ms 帧数学在任意重绘频率下精确成立（含 100ms pump tick 采样：index 恒 = floor(elapsed/40)），单测 `frame_progression_follows_the_40ms_cadence_math` 钉死； repaint 帧率维持 pump tick（100ms）为**文档化决策**——运行循环无 composer 区域级重绘源，全局 40ms 唤醒会扰动泵（共享 PTY harness 以 >80ms 静默窗切快照、已登记的 vt_temporal 时序测试钉定 100ms 节奏、空转字节量护栏），故按 brief 采用时间派生索引这一最小忠实修复；session.environment 每 session 单发（ref 效果重触发，PUT 幂等无害）；stdio.rs 模块头宣称 SIGTERM→SIGKILL 升级但实现为即杀（沿袭 rmcp 行为，措辞与 ref 2s 优雅等待不符）；project.rs:1103-1105 注释"插件 strategy 不支持"已过时（add_strategy 就在其下）；proxy.rs:367 注释"webpki 与 0.13 栈一致"在 G-26 后已失效（0.13 现走 platform-verifier；ws 链路留 webpki 为有意）。2026-11-02 第十轮备注：G-49a 仅钉定 dots 帧数学；TAB_SPINNERS 其余 3 样式（arcs/quadrants/line）、4 种未读标记与 `tabs.indicators` 设置的消费缺失不在其范围，另立 G-143。

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

---

# 第十轮（2026-11-02，全量逐行复审 + 逐项源码复核）

> 来源：`CODE_REVIEW_PARITY_2026-11-02.md`（9 通道逐行对读 + 6 轮补审 + 4 通道缺口复核：core/server、ai/client、tui、cli/schema/util），明细 `/tmp/opencode/review/verify_*.md`。对全部初报缺口逐项双侧源码取证后：**确认 60 项**（下列 G-105..G-164，续编号；各明细报告草拟编号互相冲突，一律按本节重排）、**驳回 13 项**（见文末第十轮勘误）、**DUP 归并**若干（G-6/G-17/G-32/G-49a/G-7/G-92，已就地追加备注或既有裁决覆盖）。风险分区：🔴 高 4 / 🟡 中 21 / 🟢 低 32 / ℹ️ 登记 3。

## 🔴 高

### G-105 ✅ step 流截断无收尾（fragment flush 缺口）（core）
- Rust：publisher.flush 仅 StepFinish（publisher.rs:869）与 failAssistant（:1055）触发；流在 TextStart/Delta 后**无 finish 事件也无错误事件**即终止时，unknownFinish 判定（step.rs:462-463）只认"收到未知 reason"，缺 finish 不触发；终局结算门（step.rs:548）要求 finish/failure 至少一项存在 → 开放 text/reasoning part 无 trailing delta、无 text.ended，且整步不发布 step.ended（永久悬空 part）。
- Ref：publish-llm-event.ts:132 `Effect.ensuring(flush())` 包住整条流消费，任何终止路径都收尾。
- 复现：桩 provider 发 TextStart+Delta 后直接关流。修法：流消费收尾处无条件 flush + 放宽终局门。

### G-106 ✅ CodeMode 目录指令四处偏差（codemode 组）（core）
- ① catalog 变更更新恒为整文替换：codemode.rs:868-874（注释自认 Phase 10 refinement）+ 唯一调用点 session/runner/context.rs:508；ref codemode/instructions.ts:32-124 的最小增量（新增/变更行、namespace 库存、no-longer-available 清单、与整文择短）未移植——目录每次变动模型收到全量目录文本。
- ② namespace.description 整体丢弃：codemode.rs:657,708 恒 None，namespace_line（:754-759）不渲染 ` // desc`，head 预算（:652-662）不计描述成本；ref catalog.ts:64,87,105,131,173 保留。
- ③ DESCRIPTION_LIMIT 截断按字节切片：codemode.rs:588-589 `&first_line[..117]`——多字节描述跨边界直接 panic（ref catalog.ts:69 JS slice 按 UTF-16 码元，永不 panic）。
- ④ cost 按字节除 4：codemode.rs:581-583；ref catalog.ts:188 按 UTF-16 码元——非 ASCII 内容成本被高估 1~3 倍，round-robin 提前断供。
- 复现：注册带中文描述的 Code Mode 工具即触发 ③④；目录任何增删即触发 ①。

### G-109 ✅ `PATCH /api/session/:id` 空标题行为相反（core）
- Rust：oc-server/session.rs:546-555 对任意字符串（含空串）一律发布 session.renamed（注释自认 TODO(phase-9)），空标题把会话改名为空。
- Ref：handlers/session.ts:262-271 空标题触发 `SessionTitle.generate()` 用 LLM 重新生成。
- 复现：`{"title":""}`——TS 重出新标题，rust 得到无名会话且永不恢复。

### G-118 ✅ DigitalOcean 提示缓存整体失效（provider 装配层，双层）（ai）
- 第一层：digitalocean 经 native.rs:118-120 映射后不在 facade.rs:961-973 `package_has_baked_base` 表 → 走 facade.rs:881-897 generic 分支 → `openai_chat_route`（openai_chat.rs:3220-3231，`OpenAiChatExt::base()` cache_control=false；全文件 `cache_control: true` 仅 venice:3178/openrouter:3273/alibaba:3332 三处）。
- 第二层：generic 分支 RoutePatch 不改 route id → 恒为 `"openai-chat"`，而 cache_policy.rs:39-55 的 `RESPECTS_INLINE_HINTS` 含 `"digitalocean"`（:54）却不含 `"openai-chat"`——`apply_cache_policy`（cache_policy.rs:300-302）对实际路由 no-op，`"digitalocean"` 是永无路由命中的死条目。
- Ref：digitalocean.ts:22-31 `cacheControl()` wrap 落 `cache_control` 断点。修法：补 digitalocean 专属 route，或 RoutePatch 改写 id+ext 开启 cache_control 降级。

## 🟡 中

### G-107 ✅ session.move 的 subpath 复用源 row.path（core）
- Rust：oc-server/session.rs:932-935 `subpath: row.path…`（会话**当前行**的源位置值）。
- Ref：move.ts:89 按 `path.relative(目的地项目根, 目的地目录)` 计算。
- 复现：项目 A 子目录 `sub/` 的会话 move 到项目 B 根——写入 subpath=`sub`（源值）而非 `""`。影响 session.moved 投影、会话行 path 与 location-switched 消息。

### G-108 ✅ session.move 双差异：delivery 缺省相反 + 源不可用快速路径缺失（core）
- ① rust handler（oc-server/session.rs:937-939）与 engine.move_session（engine/mod.rs:1805 硬编码 Queue 且无 delivery 参数）缺省 Queue；ref move.ts:127 `?? "steer"`——默认移动从"中断本轮转向"退化为"排队到本轮结束"。
- ② ref（move.ts:104-116,123,134-147）对不活跃且源目录消失/runner 装配失败的会话在锁内取消积压 move 项并直接 publish Moved（孤儿会话立即迁移）；rust（engine/mod.rs:1784-1811）恒走 admit→wake→等 drain，源 location 已删的会话 move 永远无法落地。

### G-110 ✅ MCP OAuth 授权流四缺口（请求/生命周期侧；G-88 已修的是返回侧校验）（core）
- ① 授权 URL 的 `resource` 参数只发 `host[:port]`（mcp/oauth.rs:765-774），RFC 8707 §2 要求绝对 URI（ref 侧 TS SDK 发完整绝对 URL）——严格授权服务器拒绝请求或签发资源绑定错误的 token。
- ② 回调不校验 `iss` 发行方参数（oauth.rs:805-837 仅查 error/state/code；ref oauth.ts:379,495 经 SDK 校验）。
- ③ token 失效无 invalidateCredentials——死凭证行不删除（ref oauth.ts:290-296 会 remove；rust registry.rs:741,769 只置 needs_auth），陈旧 refresh token 反复走注定失败的刷新。
- ④ authorize 不复用已存凭证中的已注册 client（oauth.rs:719-732 每次重新 DCR；ref oauth.ts:346-351 复用，issuer 变更才丢弃），支持 DCR 的服务器侧积累孤儿注册。

### G-111 ✅ 配对浏览器过期 401 多附 www-authenticate（core）
- Rust：oc-server/pairing.rs:83-92（:89-91）browser 分支显式 insert WWW_AUTHENTICATE——触发浏览器原生认证弹窗。
- Ref：handlers/server.ts:39-42 纯文本 401 无挑战头（刻意避免弹窗）。修法：删掉该 header insert（一行）。

### G-112 ✅ message-updater snapshot.files 语义分叉（core）
- Rust：message_updater.rs:858-862 仅 files 非空时写入，缺失/空数组均保留旧值。
- Ref：message-updater.ts:27 对 `files:[]` 写入空数组、键缺失置 undefined（清除）。
- 复现：一次 step.ended 快照树相等（files=[]）后再来一次 capture 失败（files 缺失）的终局——TS 终态 files=undefined，rust 残留第一次的列表；session.get 消息形状对外可见。

### G-113 ✅ stats 未知时区静默回退 UTC vs TS 400（core）
- Rust：oc-server/session.rs:2119-2135 valid_timezone 仅形状校验；:2616 date_key `TimeZone::get(..).unwrap_or(UTC)`。
- Ref：session/stats.ts:403-404 经 Intl.DateTimeFormat 抛 RangeError → 400 "Invalid time zone"。
- 复现：`?timezone=Invalid/Zone`——TS 400，rust 200 且日期分桶全按 UTC。

### G-114 ✅ permission saved-grant 存储吞掉全部 DB 错误（core）
- Rust：permission/sql.rs:100 `unwrap_or_default()`、:123/:144 `let _ =`——add/remove 静默丢失。
- Ref：permission/saved.ts:48,77,81 三处 `.pipe(Effect.orDie)`。
- 复现：permission DB 只读/加锁后点 "always allow"——grant 静默丢失，重启后授权消失且全程无日志。

### G-115 ✅ integration 主机三差异（core）
- ① Info.metadata 恒 None（integration/mod.rs:394,412 两处硬编码）；ref integration.ts:389 投影 entry.ref.metadata（如 `{source:"mcp"}`）——注册侧元数据在 /api/integration 读取面永远丢失。
- ② command 认证 `.output()` 等进程结束（mod.rs:904-905,920-930），stderr 不增量流入 attempt.message；ref integration.ts:645-670 双流 fork 实时更新——长认证命令期间 UI 零反馈。
- ③ oauth/command cancel 只删 attempts 表项不中断在途交换（mod.rs:761-770,1005）；ref :794,:823 `Scope.close` 中断——取消后在途 token 交换继续跑完并落库陈旧凭证。

### G-119 ✅ deepinfra provider 全线缺口（ai）
- Rust：models-dev 快照 deepinfra 条目无 `api` 字段 → facade.rs:881-887 generic 分支直接 "needs a baseURL" 报错；全仓无 `ends_with("/openai")` 追加；openai_chat.rs:1808-1900 detect 三表均无 deepinfra；facade `.model(id, None)`（facade.rs:544/1145/1285）不注入 compat。
- Ref：deepinfra.ts:10 默认 `https://api.deepinfra.com/v1/openai`、:38-44 自定义 baseURL 去尾斜杠+追加 `/openai`、:46-52 compat（max_tokens、`reasoningField:"reasoning_content"`、no-store、promptCacheKey）。

### G-120 ✅ togetherai/cerebras（含 deepinfra）无默认 baseURL（ai）
- Rust：snapshot 两家均无 `api` 字段；facade.rs:879-887 generic 分支零配置直接 Configuration 报错。
- Ref：togetherai.ts:10 `https://api.together.xyz/v1`、cerebras.ts:10 `https://api.cerebras.ai/v1` 硬编码默认。修法：补 baked 默认或在 facade generic 分支注入。

### G-121 ✅ cerebras reasoningField + 三家 promptCacheKey 缺失（ai）
- Rust：openai_chat.rs:1808-1900 detect 表无 reasoningField/promptCacheKey；compat 注入点 facade.rs:544/1145/1285 全传 None；`openrouter/venice_chat_compatibility()`（openai_chat.rs:3375-3393）仅限 baked 家族；prompt_cache_key 由 compat 门控（:2677-2679，false 则 body 永不带）。
- Ref：cerebras.ts:41-48 `reasoningField:"reasoning"`+`supportsPromptCacheKey:true`；fireworks.ts:51、digitalocean.ts:52 同。修法：facade 按 provider 注入 LanguageModelCompatibility 或扩 detect 表。

### G-122 ✅ oc-client RPC 定义层未移植（ai/client）
- Rust：oc-client/src/rpc_typed.rs 全文 140 行仅泛型 `call_into`（:30），无 per-definition 类型化方法表、无 `rpc.<id>.<event>` 事件过滤、无 on()；全仓零生产消费。
- Ref：promise/rpc.ts:53-145 `makeRpc` 从 Rpc.PortableDefinition 动态生成 141 方法表 + eventType 过滤 + on()。短期无消费方可文档化降级，否则按 oc-schema RPC 清单生成。

### G-127 ✅ composer `@` 文件模糊搜索与 `/cd` 实时目录列表未移植（tui）
- Rust：autocomplete.rs:21-25,483-484,819 三处注释自认 "the async half, not ported"；reference 模式仅剩 skills/references/agents 静态行 + 空查询 recents（:483-491 Reference 分支恒空 Vec）。
- Ref：autocomplete.tsx:336,361（directoryAutocomplete / `client.api.file.find({query, limit:20, location})`）。G-6 正文不含此面（已追加勘注）。

### G-147 ✅ 插件 ctx.model 域缺失（cli/plugin）
- Rust：oc-plugin-host/src/host/host.js:723 起 ctx 构造无 `model:` 命名空间（全 host.js 无 model.list/transform hostRequest；:625 hook 白名单里的 `model.request` 非 ctx 域）；registrations.rs 无 model editor 表。
- Ref：promise/model.ts:25-28（ModelDomain：list/get/default/transform/reload）。与 G-64（provider 域已收口）互补的半边。

### G-148 ✅ ctx.plugin.list 缺失 + effect 风格插件 API 无对应（cli/plugin）
- ① ctx.plugin.list() 未实现（host.js:723 仅 `{id}`；ref promise/plugin.ts:49 Pick<PluginApi,"list">）。
- ② `@opencode/plugin/effect` 导入面完全无对应（ref effect/ 目录 21 文件）——只 import effect 风格的插件不兼容。与 G-28（TSX 有界 shim）不同面；建议在 G-28 生态 breaking change 声明中并列记录 ②。

### G-149 ✅ auth import 把任何 credential.create 失败都当"已存在跳过"（cli）
- Rust：oc-cli/handlers/auth.rs import 段 `if client.credential().create(&body).await.is_ok()` 单分支——网络/5xx 失败也计入 skip，最终 "skipped N already present" 且 exit 0。
- Ref：auth/import.ts:11-57 仅 ConflictError 计 skip，其他错误失败。网络故障下导入静默丢失且脚本无法检测。

### G-150 ✅ CLI auth `--answer` 表单系统降级为 key=value 解析（cli）
- Rust：auth.rs:259-271 parse_answers 仅 `key=Value::String`——无交互表单（text/boolean/multiselect/external）、无 when 条件激活、无 unknown/inactive field 与 pattern 校验、布尔/数值字段无法传 JSON 值；logout/switch 无 TTY 选择器（auth.rs:806-846 resolve_account 无 target 即报错）。
- Ref：commands/handlers/auth/form.ts（answerForm 全表单面）、chooseIntegration/chooseCredential。G-61 修的是集成选择器半边，非 DUP。

### G-151 ✅ mcp auth 架构分歧 + 两行为缺口（cli）
- 架构分歧为有意设计：CLI 进程内 PKCE+loopback（mcp.rs:364-365 注释自述）vs ref 服务端 integration oauth（mcp/auth.ts）。
- 行为缺口：① 无 name 时无 mcpAuthChoices 交互选择器（TTY 也强制要求 name）；② 已有有效凭据无 re-auth 确认对话（ref 先列 connections 并 remove 旧凭据）。

### G-152 ✅ 根命令缺 UpdatePreflight 与 Env.session() 注入（cli）
- Rust：misc.rs 根 handler（:17-63）无 UpdatePreflight（ref default.ts:24 升级替换期的终端提示接管）、无 Env.session() 服务会话注入；oc-cli 无 packages.prepare / TUI updater subscribe 接线。
- 10 分钟后台更新循环本身在位（main.rs:71 spawn_background_update_check，通知走 state/update-notice.log）——初报该半边不成立。

### G-156 ✅ updater/uninstall 残留（cli）
- 包名恒 `@opencode-ai/cli`（updater.rs:81 DEFAULT_PACKAGE 常量；ref 从 release metadata.package 解析）；npm 分支无条件 `--force`（:118；ref 条件加）；vp 恒 `update`（:136）无 `install --force` 跨包迁移、pnpm/yarn 跨包迁移报错分支缺失；uninstall 不执行包管理器卸载命令（execute_removals :1104-1110 仅目录/注册/shell；ref uninstall.ts:113-125 执行 removal(method).run + win32 RetainedImage）。
- 包管理器安装检测缺失为文档化有意裁剪（updater.rs:354-357 注释，G-17 族），但显式 `--method npm` 安装后的自更新/卸载链路因此断裂。G-99 只覆盖 curl 方法语义（已修）。

### G-160 ✅ 反序列化/校验松弛三处（schema/theme）
- ① untagged OAuthValue/Disabled(bool) 接受 `true`（oc-schema/mcp.rs:97-103、config/websearch.rs；ref 仅字面量 `false`）。② resolveHue 不拒绝未知 step 键（oc-theme/resolve.rs:381-398 静默忽略 vs ref tui/resolve.ts:224 `throw "Unknown hue step"`）。③ v1 主题 null 值静默跳过（oc-theme/v1.rs:82 `Json::Null => Ok(None)` vs ref 视为已定义抛错）。畸形输入在 Rust 下更宽容，配置错误无诊断。
- 2026-11-02 第十一轮收口（lane9）：① `OAuthValue`/`WebSearchSelection` 改手写 serde（`true` → 反序列化错误 "expected … the literal `false`"，对齐 ref `Schema.Literal(false)`，测试 oauth_value_rejects_true / selection_rejects_true）；② resolve_hue_entry 对未知 step 键报 `Unknown hue step at "hue.<name>.<step>"`（对齐 ref resolve.ts:222-225，测试 unknown_hue_step_errors）；③ v1 `Json::Null` 改抛 `Invalid V1 theme color`，variant 的 mode 键存在但为 null 同样抛错、present-but-null 的 `selectedListItemText` 视为已定义（ref v1-migrate.ts:354,367）；`backgroundMenu` 按 ref 真值语义（null → 回退 backgroundElement，v1-migrate.ts:372），测试 null_values_error。

## 🟢 低

### G-116 ✅ PTY 服务三差异（core）
- ① 输出 cursor/buffer_cursor 按字节计数（pty.rs:176,:188）；ref pty.ts:206 按 UTF-16 码元——多字节输出的会话 cursor 经 WS/跨进程传递与 v2 客户端不可互换（回放跳段或重复）。
- ② 订阅回调 panic 无隔离（pty.rs:173-183 持锁裸调；锁为 parking_lot 不毒化，但 panic 沿 reader 线程 unwind 致数据流静默中断、会话卡 running）；ref pty.ts:214,300 catch 后移除订阅者继续。
- ③ win32 的 LC_ALL/LC_CTYPE/LANG=C.UTF-8 注入缺失（pty.rs:459-460 仅 TERM/OPENCODE_TERMINAL；本构建非 win32 目标，低危前瞻项）。

### G-117 ✅ 凭证存储两处微偏差（core）
- activate/update 额外更新 time_updated（integration/credential.rs:239,:285-297；ref credential.ts:124-130,227-228 的 set 不含该列）——外部可见 mtime 漂移。
- 无 integration_id 的行以 `integration_id=""` 进入 all() 结果（credential.rs:71,:132 `unwrap_or_default()`；ref :69 直接丢弃）。复现：手动插入 NULL integration_id 凭证后 `credential list`——ref 隐藏，rust 返回空串条目。

### G-123 ✅ togetherai/digitalocean env 回退链缺失（ai）
- Rust：snapshot togetherai env 仅 `["TOGETHER_API_KEY"]`、digitalocean 仅 `["DIGITALOCEAN_ACCESS_TOKEN"]`；facade.rs:551-558 bearer_auth 只按 catalog env 顺序构造。
- Ref：togetherai.ts:37 `[TOGETHER_API_KEY, TOGETHER_AI_API_KEY]`；digitalocean.ts:36 `[DIGITALOCEAN_ACCESS_TOKEN, DIGITALOCEAN_API_KEY, DO_INFERENCE_API_KEY]`。

### G-124 ✅ media Generation 缺 events()/await() 观察流（ai/client）
- Rust：oc-ai/src/media/route.rs:812-905 仅有 token/first_snapshot/status/result/cancel/poll_read；Observation 事件类型在 media/model.rs:65-144 有定义但 oc-core/oc-server 零消费。
- Ref：generation.ts:111-145 `events()`（schedule 轮询推送 queued/progress/finished，含 deadline 钳制）+ resultEvents。补 events() 或文档声明降级。

### G-125 ✅ oc-client SharedEvents 终止错误只投递一次（ai/client）
- Rust：shared_events.rs:341-344 err_yielded 守卫——只产出一次 Err 后流结束（注释自认刻意偏离，ClientError 非 Clone）；terminal_error()（:330-334）为补偿 API。
- Ref：shared-events.ts:76,123 完成时 reject 全部 pending、completion.error 后每次 next() 都 reject。跨语言消费者按 TS 语义写的循环会提前退出。建议文档标注契约差或改投递 Arc<ClientError>。

### G-126 ✅ SSE 16MiB 上限按字节 vs TS 按字符（ai/client）
- Rust：sse.rs:12 MAX_SSE_EVENT_BYTES、:39/:200 按字节计数。
- Ref：client.ts:385-386 按解码后 JS 字符（UTF-16 码元）。含 CJK/emoji 的长事件 Rust 提前报 SseEventTooLarge。按 chars().count() 或文档钉定即可。

### G-128 ✅ 通用 spring/tween 动画引擎未移植（tui）
- Rust：全 crates 无 createAnimatable/advanceSpring；session_tabs.rs:14 注释 "motion springs stay C2"；tool_detail.rs websearch 静态渲染。
- Ref：ui/animation.ts（advanceSpring）+ retry-provider.tsx:19-20（FADE=80/RESIZE=60）。连带丢失：RetryProvider fade/resize、tab 宽度 spring、pane-resize 平滑过渡、title-shimmer/DIM 回退（session_tabs_solid.rs:773-795 静态渲染，无动画分支）。

### G-129 ✅ display_width 缺文本呈现型 emoji 收窄规则（tui）
- Rust：wrap.rs:89-91 `text.width().saturating_sub(merged_cells(text))`（unicode-width；merged_cells 只处理 Brahmic）。
- Ref：string-width.node.ts:9-26（width==2 且非 EAW2 且 textEmoji 且无 VS16/20E3/ZWJ → 记 1）。❤ 等字符 Rust 记 2、TS 记 1。

### G-130 ✅ splitAddedPatch 未移植（tui）
- Ref：util/diff.ts:18-38（新增文件大补丁按 size 切块）。Rust 全 src grep `-0,0`/split_added 零命中，diff_viewer.rs 直接解析 hunk。

### G-131 ✅ dialog-select alt+↑/↓ 跨 section 跳转未移植（tui）
- Ref：dialog-select.tsx:367-374,547-562（sectionNavigation）。Rust dialog_select.rs grep alt+up/move_section 零命中。

### G-132 ✅ 导出选项交互表单折叠为 4 静态行（tui）
- Rust：command_palette.rs:1051-1059 四条 SelectOption，描述写死 "thinking off, tool calls on"/"unsanitized"。
- Ref：dialog-export-options.tsx:11-42（defaultThinking/thinking/tools/sanitize 交互态）——用户无法配置导出内容。

### G-133 ✅ openUrl 丢失 scheme 校验；仅 xdg-open（tui）
- Rust：store.rs:8519-8531 任意 scheme 直接交 xdg-open（无 http/https 校验）；dialog.rs:1697-1706 openPath 同款。
- Ref：link.tsx:27 → util/open.ts:3-12（npm open 三平台 + openUrl 校验 scheme）。openPath/openUrl 仅 xdg-open 在 Linux 目标可接受（建议文档化）；scheme 校验是真缺口。

### G-134 ✅ mini 完成态工具结构化快照未移植（tui）
- Ref：mini/tool.ts（1358 行：toolView/snapWrite/snapEdit/snapPatch/snapTask/toolScroll、代码块/diff 卡/任务卡/可滚动展开）。Rust mini/ grep 零命中；transport.rs tool_caption 自述 "caption subset"，仅保留行内 caption。

### G-135 ✅ model-preference setFavorite 写路径缺失（tui）
- Rust：mini/model_pref.rs 仅 ModelPreferenceDocument{recent,favorite,variant}（:117,126 favorite 只读保留，无 set_favorite）。`model.dialog.favorite` 键位已注册但收藏永不持久化。Ref：model-preference.ts setFavorite。

### G-136 ✅ Route 枚举无通用 prompt 携带字段（tui）
- Rust：store.rs:47-50 `enum Route { Home, Session { session_id } }`（仅 app.rs:215 arm_startup_prompt 专用通道）。Ref：context/route.tsx:12-19（HomeRoute.prompt/SessionRoute.prompt）——任意导航时预填草稿不可达。

### G-137 ✅ `session.toggle.thinking`/`scrollbar` 键位注册但无 dispatcher 臂（tui）
- Rust：oc-tui src grep `session.toggle.thinking|toggle_scrollbar` 零命中（对照 verbosity.cycle 已在 command_palette.rs:247-252 正确接线）。Ref：index.tsx:1024-1049 写 config.update。按键无任何效果。

### G-138 ✅ session.undo 缺 pending 分支（tui）
- Rust：store.rs:7721-7762 session_undo_pressed 无条件 revert_stage。Ref：index.tsx:985-992（pendingDeliveries.has → mutatePending("cancel")，否则 interrupt→wait→revert.stage）——仍在 inbox 排队的消息被错误走 interrupt+revert.stage 而非 inbox.cancel。

### G-139 ✅ crossterm 键翻译 CONTROL 分支只匹配 Char（tui）
- Rust：app.rs crossterm_key_to_spec:9133+ `match key.code { Char(ch) => …, _ => None }`——ctrl+shift+tab（BackTab+CONTROL）返回 None，`session.tab.previous` 主键位 PTY 路径失效（alt+up 别名可用）。

### G-140 ✅ session 路由长尾组（7 合 1）（tui）
- ① AssistantRetry 重试倒计时 footer 未建模（Message 无 retry 字段）；② TurnTokenUsage 调试表缺失；③ Write/Edit 后 LSP Diagnostics 行缺失；④ 导出转录裁掉工具 Input/输出正文（app.rs:6388 仅 `**Tool: name**` vs ref index.tsx:3503-3533 含 Input JSON+输出全文）；⑤ pending-steer 点击管理（Move to queue/Delete）缺失（app.rs:1120-1126）；⑥ --auto 权限失败不回滚 autoApproved（rust fire-and-forget vs ref index.tsx:266）；⑦ session.redo 无 enabled 门（ref index.tsx:169-175 revert?.messageID 条件）。

### G-141 ✅ 嵌入式 PTY 面板主题调色板注入未移植（tui）
- Rust：terminal_pane.rs grep osc/palette/oklch 零命中。Ref：terminal-pane.tsx:338-385（OSC 4/10/11 + rgbToOklch 色相匹配）——PTY 输出用终端默认配色。

### G-142 ✅ cliErrorMessage 友好错误格式化链未移植（tui）
- Rust：grep cliErrorMessage/"Did you mean" 零命中（app.rs error_to_string 原始 to_string 进 toast）。Ref：util/error.ts:5-26（递归 + "Did you mean: …" 建议、ConfigInvalidError issue 明细）。

### G-143 ✅ tabs spinner 仅 dots 样式；indicators 设置不生效（tui）
- Rust：session_tabs.rs:37,42 仅 dots（注释 "TAB_SPINNERS.dots"）；dialog_config.rs:243 有 tabs.indicators 设置行但 config.rs 不解析、渲染器无 numbers 分支。
- Ref：TAB_SPINNERS 4 样式（arcs/quadrants/line 缺）+ 4 种未读标记。G-49a 只钉定 dots 帧数学（已追加备注）。

### G-144 ✅ autocomplete 触发检测用 char 偏移而非显示宽度偏移（tui）
- Rust：autocomplete.rs:513-546 trigger_for 基于 `Vec<char>` 索引。Ref：prompt/display.ts:41-61 slashTriggerIndex/mentionTriggerIndex 基于 displaySlice + promptOffsetWidth。语义等价，但宽字符混排时 @ // 触发锚点可能与 ref 差 1+ 列（需实测）。

### G-145 ✅ 插件渲染崩溃隔离降级 + activity-summary 未实现（tui）
- ① 插件渲染失败 eprintln 降级，无 "crashed in slot" 错误 toast（ref ErrorBoundary）。② activity-summary 独立视图仅存注释引用（message/list.rs:64-68），低 verbosity 的工具/思考折叠摘要未实现。（未知键位 log+skip+toast 半边为已文档化有意偏离，不立项。）

### G-146 ✅ dialog-config "Color mode" 设置搜索关键词为空串（tui）
- Rust：dialog_config.rs:91-98 keywords: ""（相邻 colors 条目有 "color scheme, colors"）。Ref：dialog-config.tsx:35 `["dark mode","light mode","system theme"]`——palette 搜 "dark mode" 不命中。（title-shimmer/DIM 半边并入 G-128。）

### G-153 ✅ plugin list 不发现本地目录插件；版本截断不校验 hex（cli）
- Rust：plugin.rs list 段无 localPluginDirectories/localSource；:60 `version.len() == 40 || == 64` 只看长度（任意 40 字符版本串被截 7 位）。Ref：plugin/list.ts:14（本地发现）、:94 正则 `/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/i`。

### G-154 ✅ api/models 输出差异（cli）
- Rust：misc.rs models 段 `models.sort()` 字节序；api.rs 仅 Basic auth。Ref：models.ts:18 Service.headers、:23 localeCompare；api.ts:29 同。oc-util 已有 localecmp.rs 却未消费。

### G-155 ✅ stats 输出降级（cli）
- ① 活动日历网格/图例/COLORFGBG 亮暗调色/ANSI 颜色缺失（stats.rs:2 注释自述 REGISTERED simplification；ref stats.ts:165-194,93,100,319-342）；② "best streak" 在 activity 行与 tool 行重复输出两次（:288,:302）；③ 日界 UTC（:123 rem_euclid 86400000、TZ||"UTC"）vs ref 本地午夜 + Intl IANA（stats.ts:40,144-163）——非 UTC 时区天数桶起点可能偏移；④ 数字无千分位。

### G-157 ✅ mini 三偏差（cli）
- ① 不 chdir 到 $PWD（mini.rs:203-211 仅字符串采纳；ref mini.ts:201-209 真 chdir 失败报错）——相对路径语义不同。② reconnect seam 恒 None 永不重连（mini.rs:234；ref handlers/mini.ts:29 接 250ms 重连循环）。③ 新增 30s 总 deadline（mini.rs:126；ref 无限重试直到 abort）。G-68（fragment/image 回放，已修）不含本条。

### G-158 ✅ server-connection 无 onStart；standalone 无 Options.command（cli）
- server_connection.rs grep onStart/Starting background 零命中（ref server-connection.ts 有 "Starting/Restarting background server..." 提示回调）；standalone.rs:116-117 恒 self_command（无自定义命令）、:160 新增 60s ready 超时（ref 无限等待）。

### G-159 ✅ ACP getSession 目录匹配用 canonicalize（cli）
- Rust：acp/service.rs:379-381 双侧 `std::fs::canonicalize`（解析 symlink）。Ref：acp/service.ts:100 `FSUtil.resolve`（词法归一）。经符号链接进入的会话目录匹配语义不同（Rust 更宽松）。建议注释钉定或改词法归一。

### G-162 ✅ util 残留两小项（util）
- ① npm registry 解析仅 4 层级联（npm_config.rs:23-29 env→项目→用户→默认）；ref 走 @npmcli/config 全量（globalconfig/builtin 等）——企业全局 .npmrc/镜像配置行为不同。② Hash.fast(sha1) 无统一 util（hash.rs 仅 sha256_hex，sha1 散落调用点各自引入）。glob brace/extglob/scan 见 G-92 已钉定。
- 2026-11-02 第十一轮收口（lane9）：① 级联扩为 env → 项目 → 用户 → globalconfig（`$PREFIX/etc/npmrc`）→ builtin（`$PREFIX/lib/node_modules/npm/npmrc`）→ 默认（npm_config.rs 模块头文档化）；有界偏离：`PREFIX` 仅取 `NPM_CONFIG_PREFIX`/`npm_config_prefix` 环境变量（@npmcli/config 会从 node/npm 安装推导，本端口不 spawn `npm config` 子进程），未设 prefix 环境变量时跳过 global/builtin 两层（测试 globalconfig_after_user / no_prefix_env_skips_global_levels）。② oc-util hash.rs 新增 `sha1_hex`，迁移机械调用点：oc-cli util.rs `hash_fast`、service_config.rs default_port/legacy_filename、oc-core project.rs `hash_fast`、snapshot.rs（删除手写 SHA-1 实现）、engine/skills_url.rs（弃 ring SHA1）、tests/database_identity.rs；oc-cli/oc-core 的独立 sha1 依赖随之移除（统一走 oc-util）。有界偏离：oc-ai（SDK crate，不依赖 oc-util）bedrock credentials 的 AWS SSO 缓存名 sha1 保留本地 `sha1` 依赖（非 `Hash.fast` 语义，不迁移）。

## ℹ️ 登记

### G-161 ✅ Global roots 两记录性偏差（util）
- ① APP_NAME="opencode-rust"（global_roots.rs:29；ref global.ts:31 "opencode"）——有意共存设计（legacy_v2_roots 提供迁移源，已有注释），补文档登记。② `Roots::ensure`（:505-519）不做 tmp realpath 规范化（ref global.ts:92-93 以 canonicalTmp 为比较基准）——macOS /private/tmp 与 /tmp 混用时路径比较可能失配，建议 canonicalize 或注释钉定。
- 2026-11-02 第十一轮收口（lane9）：② 已修——`Roots::ensure` 签名改为返回 `io::Result<Roots>`，建目录后对 tmp 做 `fs::canonicalize`（失败回退原路径），对齐 ref acquire 的 realpath 语义（测试 ensure_canonicalizes_tmp：符号链接 tmp 规范化后相等）。① 维持有意共存设计，本条登记为文档化决策（migration source = legacy v2 roots）。

### G-163 ℹ️ workerd 运行时目标与 ServerFetch.make 不移植（server）
- Rust：grep boot.rs/oc-server 无 workerd/ServerFetch，仅原生 `boot_and_serve`（axum）。Ref：server/src/fetch.ts（web-standard handler 嵌入）、workerd.ts（运行时替换层）。结构性不可移植（无 JS 运行时嵌入目标）；已收编 G-17 族文档化范围裁剪（本条为登记，见 G-17 追加备注）。
- 2026-11-02 第十一轮：登记收口——维持 G-17 族文档化范围裁剪裁决，Rust 侧仅原生 boot_and_serve（axum）为最终形态，无后续工程项。

### G-164 ℹ️ announce.rs 为位置事件级联的服务端替身（server）
- Ref 由各 core 服务自持 notify（reload/transform → bus.publish）；Rust 由 oc-server/announce.rs 按装配/请求时机合成（模块头 :1-15 自述；engine/mod.rs:652 F-5 注释佐证；含 project.updated 60s 限速、九个空 *.updated 帧复刻，有 parity 测试）。结构性 seam：新增 location 服务/事件需同步维护 announce 级联清单，漂移风险集中该文件——保持 parity 金样并在此登记。
- 2026-11-02 第十一轮：登记收口——结构性 seam 维持现状；新增 location 服务/事件时必须同步维护 announce.rs 级联清单并保持既有 parity 金样（约束性登记，无代码项）。

## 归类与收编（不另立项）

- **experimental AI（evaluation/system-one）** → G-7（附 G-17），第九轮同款裁决。
- **`POST /api/plugin/install` 客户端方法** → G-17 rust-extra 家族（第五轮已裁决，methods.gen.rs:2201 与服务端路由同源）。
- **tui feature-plugins/system/storybook 工作台** → G-17（开发期 fixture 同族，已追加备注）。
- **run 非 TTY 权限行为** → G-17（TTY 提示为有意增强，非 TTY 路径 auto-reject 与 ref 一致）。
- **run/v1.ts 输出兼容层** → G-32（v1/legacy 族，已追加备注）。
- **schema 五子项（rpc.ts/legacy-event/durable-event-manifest/models-dev 管道/v1/*）** → G-32 明文范围（G-87 已裁决，已追加备注）。
- **glob brace/extglob + Glob.scan** → G-92（注释级钉定）。
- **未知键位 log+skip+toast**（tui #20 半边）→ 已文档化有意偏离（代码注释+测试），非缺口。

## 第十轮勘误（经复核撤销的初报，勿再立项）

1. **Formatter 为 no-op** — formatter.rs 同时含生产 `BuiltinsFormatter`（:384 真实命令执行，:779 impl；接线 engine/mod.rs:1241）与仅测试用 NoopFormatter（:99）；formatter.rs:1-7 模块头"ships a no-op"为陈旧注释（建议顺手清理）。
2. **git clone/fetch/checkout/resetHard 与 vcs.ts/patch.ts 未移植** — clone/fetch/checkout -B/reset --hard 全在 oc-core/reference/repository_cache.rs:241-290；vcs 全家在 oc-server/handlers/project.rs:968-1818（split_git_patch :1709、countPatch/addPatch :853-880）；第五轮勘误 #5 结论维持。
3. **MCP reconcile 用 PartialEq 替代深度相等** — ServerConfig（oc-schema/mcp.rs:103-109）全类型化（String/BTreeMap/bool/u16/枚举），derive PartialEq 即结构化深度相等且 BTreeMap 键序恒定，与 isDeepStrictEqual 等价。
4. **子代理 prompt 绕过 SessionPrompt.prepare** — tool/subagent.rs:368-377 裸 admit，但 drain 消费边界统一兜底：runner/drain.rs:487,524-543 → prompt_prepare.rs:141 对所有 pending user 行跑 hook + @skill（第五轮勘误 #3 同族裁决）。
5. **isTransientError 丢 cause 链遍历** — oc-util/retry.rs:144 在全仓零生产调用；唯一生产瞬态分类器 MCP 侧自建链完整（mcp/client.rs:273-276 沿 source() 收集、:1375-1377 chain.join 进错误消息）。
6. **Flock 心跳/可观测性丢失** — flock(2) 崩溃恢复语义严格强于用户态锁目录协议（内核自动释放），heartbeat/stale-breaker 无行为后果；互操作边界已由 oc-tui/storage.rs:44-49 显式文档化。
7. **服务生命周期整套未移植** — 已移植在 oc-server/src/lifecycle.rs（2599 行）：discover/probe（:86-385）、ensure 三次超时强杀（:936-952）、版本不匹配重建（:980-999）、SIGTERM→SIGKILL 升级（:1115-1160）；初报只扫了 oc-client。
8. **pty-handoff sidecar 缺失** — 生产侧 lifecycle.rs pty_handoff_prepare/environment/complete/clear（:1235,:1010,:1356,:1365）、消费侧 boot.rs:774-775 读取并清除 OPENCODE_PTY_HANDOFF（:989-1065 测试），四函数一一对应。
9. **AbortSignal 取消全链路缺失** — Rust drop future 即中止在途请求（reqwest/SSE 语义等价）；SharedEvents 有显式 stop()→task.abort()（shared_events.rs:212-217）；timeout 包调用即得 deadline。
10. **pty connect 两 endpoint 生成为 execute_json** — 功能由 oc-tui/app.rs:6585 完整承接（connect-token→ticket→WS 升级流）；死封装自带 TODO(pty-ws) 注释钉定（methods.gen.rs:1959-1960,2533-2534）。
11. **connect_token Envelope 解包丢 location** — 初报配对错位：persistent-pty endpoint 两侧同取 data（TS types.ts:6382 同样只取 data）；pty endpoint 两侧都含 location（types.gen.rs:9898-9905 vs TS types.ts:6280 不解包）。逐 endpoint 全等。
12. **可执行 Tool 运行时不在 oc-ai** — 跨 crate 归属：oc-core/src/tool/registry.rs 完整承接（模块头 :2-6 自述 port of runtime.ts execute + tool.ts executeTool，:192 注释明言），非缺失。
13. **serve 缺 PTY handoff / EADDRINUSE 15s 重试** — 均在 oc-server：lifecycle.rs:1334-1377（handoff 继承+decode_inherited_handoff，boot.rs:776 接线）、:1478+（incumbent 100ms×15s 重试环）+ :1505-1512 逐字复刻的 "Managed service port…" 文案；serve.rs 经 boot::boot_and_serve（:143）全链路接线。

## 第十轮判定汇总
初报 79 项：确认立项 60（G-105..G-164：🔴 4 = G-105/106/109/118；🟡 21 = G-107/108/110-115/119-122/127/147-152/156/160；🟢 32；ℹ️ 3 = G-161/163/164）；驳回 13（见勘误）；DUP 归并 6 处（G-7/G-17×3/G-32×2/G-92 + G-6/G-49a 勘注）。core/ai 侧共同根因（G-118..G-121）：provider 目录装配层是 parity 录制盲区——facade generic 分支 + detect_* 表 + models.dev 快照三处都没有 TS provider 文件里 hardcode 的 baseURL/env 回退/compat 注入，建议为受影响 provider 补 baked 默认与 `*_chat_compatibility()` 表（复用 openai_chat.rs:3375-3393 既有模式）。

## 第十一轮 burndown 收口（2026-11-02，gap-burndown-7）

第十轮立项的 G-105..G-164 全部 60 项经 lanes 1-9 串行落地并合并 main（worktree 隔离、dev-gate、nextest `--workspace --locked --no-fail-fast` 证据、合并门复跑；日志在 `.test-logs/*-lane9*` / 各 lane 报告）。逐 lane 提交：

- lane1 `ce86cd7`：G-105/106/109（+ G-106 组内 CodeMode 目录指令收口）。
- lane2 `a3485a4`：G-118..G-123。
- lane3 `ed23543`：G-107..G-115。
- lane4 `b0a026f`：G-147..G-156。
- lane5 `56e8d36`：G-127..G-134。
- lane6 `e8c3b7f`：G-135..G-146。
- lane7 `18363aa`：G-122/124/125/126。
- lane8 `34837c8`：G-154/155/157/158/159。
- lane9（本轮）：G-160/162 代码修复 + G-161/163/164 登记（合并 main，见文末 git log）。

状态：G-105..G-164 逐条改标 ✅（代码落地或登记收口）；G-163/G-164 维持 ℹ️（结构性登记），G-161 ② 已修 ① 文档化。第十轮判定汇总中的 🟡/🔴 分级至此全部清偿。

### 各 lane 有界偏离（bounded deviations）清单

- **G-105**：step failed-not-ended 的终态形状提升（hoisted）到统一收尾路径——与 ref 逐分支 inline 形状不同但终态集合等价。
- **G-106**：UTF-16 近似（按注记点用码点/字节近似 UTF-16 码元语义）。
- **G-108②**：仅目录已消失（directory-gone）快速路径；其余源不可用场景走常规失败分支。
- **G-122**：validated dispatch（定义级校验派发）而非 per-fn codegen——RPC 定义层等价、无逐函数生成物。
- **G-124**：stream seam——events() 观察流上游不消费（poll 驱动边界在 seam 注释钉定）。
- **G-128**：动画 pump 以 100ms 粒度驱动（ref rAF 帧）；spring/tween 数学一致。
- **G-129**：静态 emoji 表（text-presentation emoji 收窄规则以表驱动近似 ref 运行时判定）。
- **G-134**：无虚拟化/语法高亮（mini 完成态结构化快照渲染为静态卡片）。
- **G-136**：PromptInfo 仅文本承载（无 ref 的 Route.prompt 结构化字段族）。
- **G-140**：折叠摘要（7 合 1 长尾组按子项分别落地，部分交互为摘要级）。
- **G-141**：OSC 输入流注入（调色板经 OSC 4/10/11 注入，rgbToOklch 匹配为静态实现）。
- **G-143**：仅复刻 ref 默认样式（dots）；其余 spinner 样式与 indicators 设置不生效维持裁剪。
- **G-147**：快照式 editor（插件 ctx.model 域以快照提供，非 ref 的实时视图）。
- **G-148②**：effect API 仅 clear-error 钩子（ref 的 effect 风格插件 API 子集）。
- **G-150**：multiselect 自定义控件 once-per-session（会话级一次性渲染约束）。
- **G-153**：有界本地发现（localPluginDirectories 的子集扫描）。
- **G-156**：包名探针回退（release metadata.package 缺失时回退 DEFAULT_PACKAGE 探测）。
- **G-158**：Options.command 纯 seam（standalone 自定义命令以无副作用的 seam 暴露，默认仍 self_command）。
- 本轮（lane9）新增有界偏离：G-162① npmrc 级联的 `PREFIX` 仅取环境变量（不 spawn npm，不推导 node 安装前缀）；G-162② oc-ai bedrock SSO 缓存名 sha1 保留本地依赖（SDK crate 不引 oc-util）；G-161② canonicalize 失败回退原路径。

现存未关闭项仍均为文档化决策（G-17 族、G-28、G-32 等），无新增待办。


---

## 第十二轮（2026-11-03 全量对比审查）新登记缺口（G-165..G-210）

> 背景：22 条并行审查通道对端口范围内全部包（core/tui/cli/client/server/schema/theme/plugin/util/ai/codemode/merman）做逐文件全文对读（TS ~21 万行 / Rust ~40 万行生产代码），再经 3 条独立核验通道对 50 项缺口声称逐条双侧源码取证：46 项立项、4 项撤销、若干子句修正，全部与既有 G-1..G-164 去重。总报告 `CODE_REVIEW_PARITY_2026-11-03.md`，域报告与核验记录 `/tmp/opencode/review/`（session/tools/ai_*/plugin/mcp_auth/shell_pty/config/schema/bus_db/fsvcs/server/client/cli/theme_util/codemode/core_misc/tui_*/render_mermaid + verify_v1..v3）。

### 🔴 高

### G-165 ❌ Azure provider catalog 通道整体未接线（ai）
- facade.rs:346-347 把 catalog `azure` 归入通用 OpenAIResponses 族；:795-801 仅 `bearer_auth`（env 链把非密钥 `AZURE_RESOURCE_NAME` 当凭据候选，snapshot env + catalog.rs:255 + facade.rs:556-562），无 api-key 头/api-version/resourceName/deployment URL。
- openai_responses.rs:42,573 无 baseURL 兜底 → 未配 baseURL 静默错发 `https://api.openai.com/v1`；custom 通道有 azure 守卫（facade.rs:1362-1370 fail-fast）但 catalog 通道无。
- oc-ai/protocols/azure.rs（392 行，azure_auth:103-109 / resource_base_url:40 / endpoint_parts:123）生产零调用；open_responses_channel.rs:424-425 虽引用其 WS helper，但 `responses_channel_enabled`(azure.rs:177-192) 要求 `.openai.azure.com`+`api-version=v1`，generic 路由永不满足 → 事实死代码。ref 基准 ai/src/providers/azure.ts:29-33,113-128。

### G-166 ❌（wire bug）`Mcp.Protocol` serde 变体名大写（schema）
- oc-schema/src/mcp.rs:26-35 枚举无 `rename_all`（仅 Dated 有 rename="2026-07-28"）→ wire 输出 `"Legacy"/"Auto"`；ref schema/src/mcp.ts:20 为小写 literals。
- 该类型即 v2 config 类型（config/mcp.rs:20 re-export、config/normalize.rs:28 消费）→ `"protocol":"auto"` 反序列化失败。修法：`#[serde(rename_all="lowercase")]`（Dated 变体保留显式 rename）。

### G-167 ❌ Copilot anthropic 车道 + 动态模型面整体缺失（ai）
- **anthropic 逃逸通道缺**：ref 对 advertise `/v1/messages` 的模型由 models.ts:164 将 `package` 改写为 `@opencode/ai/providers/anthropic` 实现 lanes（github-copilot.ts:325-342 的 hook 不消费 "messages"）；rust copilot_responses.rs:94-101 分发不认 messages 落到 chat lane，且 CopilotModelEntry（copilot.rs:327-341）无 package 字段 → Claude-on-Copilot 错发 /chat/completions。
- **动态模型同步死代码**：load_copilot_models/derive_copilot_models（copilot.rs:347,398）全仓仅 tests/copilot_models.rs 引用；facade 唯一 copilot import 是 copilot_chat_route（facade.rs:45）→ 新模型/别名/enterprise 端点不可见，仅静态 catalog。
- **派生字段缺**：ref models.ts:129-196 另产 family/modalities/variants/released/cost(AIC→USD)/limit；rust 仅 id/model_id/name/enabled/endpoint/base_url。
- **gpt-5-chat-latest 未隐藏**（ref github-copilot.ts:259-268 enabled=false，rust 零命中）；**免费标题通道 utilityTitle（gpt-4o-mini）未移植**（ref :398,415-475 + title hook :455-475）。

### G-168 ❌ 第三方 Anthropic 根 URL 拼错（moonshot/zai-coding-plan/alibaba）（ai）
- rust catalog.rs:192-199 把 snapshot `api`（chat 根）写入 settings.baseURL；AnthropicMessages 臂（facade.rs:787-792）仅 `with_base_url`，PATH="/messages"（anthropic_messages.rs:59）→ 拼成 `{chat根}/messages`。
- ref 基准：moonshot.ts:86 `api.moonshot.ai/anthropic/v1`；zai-coding-plan.ts:45 `api.z.ai/api/anthropic/v1`；alibaba.ts:94-98 `${host}/apps/anthropic/v1`。minimax 不受影响（其 catalog api 本身是 anthropic 根）。facade 无 per-package anthropic 根特判（仅 bedrock-mantle :644 有）。

### G-169 ❌（🔴 安全）shell 权限扫描：注释后换行被跳过 → 命令合并逃逸权限分类（core）
- rust shell/scan.rs:1841-1848：`#` 注释分支 `index = newline + 1` 跳过换行；ref scan.ts:584-588 是 `index = newline - 1`（循环尾 ++ → 换行被访问，走 scan.ts:590 的 `;<>&|\n` 分隔符分支结束当前命令）。
- 后果：`echo hi # note\ncurl evil` 在 rust 合并为**单条命令** `echo hi curl…`，save 前缀按 ARITY 取 `echo *` → 注释后一行任意命令逃避独立权限分类；且注释行处 pending heredoc 不在该换行读取。修法：改处理换行（等价 ref `index = newline` 再走分隔符路径）。

### G-170 ❌ session transfer export 脱敏弱于 ref（core）
- rust 服务端有 export 端点（oc-server/handlers/session.rs:2142-2174，`?sanitize=true`），但 sanitize 仅删 info/messages 的 `metadata` 字段（:2163-2172）；oc-core session/transfer.rs 只有 import（:34-70）无 export 服务。
- ref transfer.ts:165-230 的 isSettled 过滤（:60,165-170）、redact 占位体系（:171-176）、title/directory/revert/text 脱敏（:180-230）全部缺 → sanitize 导出仍包含 ref 会脱敏的正文内容。与 G-75（CLI 面）互补不冲突。

### 🟡 中

### G-171 ⚠️ OpenAI/Azure gpt-5 默认选项 + chat 线 promptCacheKey 缺（ai）
- ref openai-options.ts:35-47 `gpt5DefaultOptions`（reasoningEffort:"medium"/reasoningSummary:"auto" + gpt-5 门控排除 gpt-5-chat/pro）rust 零命中；openai.ts:103-106 两 lane 均应用。
- chat 线 promptCacheKey：ref openai.ts:108/azure.ts:132 `compatibility:{supportsPromptCacheKey:true}`；rust openai/chat 绑定（facade.rs:891-897）无 compat → openai_chat.rs:2683-2686 `unwrap_or(false)` 丢弃。
- 已按 route 默认落地的不算缺：store:false + include reasoning.encrypted_content（openai_responses.rs:534-552 = ref route defaults）。G-121 根因扩展。

### G-172 ⚠️ MiniMax chat / Meta messages 专用协议未移植（ai）
- ref minimax.ts:58-75,91：`thinking` 选项 + `reasoning_split` 默认 true（防 `<think>` 内嵌）+ compat{store/strict:false}；rust minimax/chat → 泛化 OpenAIChat（facade.rs:383），oc-ai 无 minimax overlay。
- ref meta.ts:113-118 + meta-messages.ts:44-45：MetaMessages（web_search 降级、拒 searchContextSize、thinking adaptive 默认）+ requireSignature:false；rust meta/messages → 泛化 AnthropicMessages（facade.rs:334）。

### G-173 ⚠️ ToolSchemaProjection 未中央化（ai）
- ref route/client.ts:565 在 prepareRequest 对**所有**路由统一应用 objectRoot + 按模型名清洗；rust `project_tools` 仅 gemini.rs:1380 / mistral_chat.rs:525 / cohere_chat.rs:146 调用，anthropic_messages.rs:574、openai_chat.rs:1982、openai_responses.rs:648、bedrock/converse.rs:252 裸用 `input_schema` → 无 object 根的 schema 与 kimi 清洗在主链路缺失。修法：在 Route::prepare_request 统一调 sanitizer::resolve(model)::project_tools。

### G-174 ⚠️ requireSignature 语义靠启发式（ai）
- ref 显式 `requireSignature:false`：alibaba.ts:104 / meta.ts:141 / zai-coding-plan.ts:72 / vercel-ai-gateway.ts:264（仅 anthropic/ 前缀 true）；rust anthropic_messages.rs:1053-1080 仅 kimi/moonshot/xiaomi 启发式 → 四家全落 true，无签名 thinking 回放失败。
- compat 钩子在协议侧存在（:1055-1060 读 `compatibility.require_signature`）但无注入方：facade package_chat_compatibility(:1063-1072) 不含 messages 族、overlay_model_compatibility(:1096-1108) 不转发、oc-schema model.rs:71-86 Compatibility 无该字段 → 配置亦无法表达。

### G-175 ⚠️ chat 族 compat preset 注入不全（12+ 家）（ai）
- facade.rs:1063-1072 仅注入 deepinfra/togetherai/cerebras/fireworks/digitalocean 五家（=G-119/G-121 修复成果）；专用工厂（facade.rs:871-887）与 chat_route 工厂（openai_chat.rs:3198-3223）均不带 compat，deepseek/groq 等 preset 函数仅 `#[cfg(test)]` 引用（openai_chat.rs:3626-3633）。
- ref 注入 compat 的 chat 族：deepseek.ts:47 / groq.ts:92-98 / openrouter.ts:199 / venice.ts:43 / zai.ts:51 / zai-coding-plan.ts:68 / alibaba.ts:97 / minimax.ts:119 / moonshot.ts:111 / cohere.ts:43-48 / openai.ts:108 / azure.ts:132 / meta.ts:136 —— 全部未注入。G-118..G-121 根因扩展。

### G-176 ⚠️ flowchart 汇流排分组键=edge_index（3/4 臂失效）（tui）
- routing.rs:899-905 `source_key()/target_key()` 恒返回 `self.edge_index` → 每组单元素 → `len()<2` 恒跳过；失效臂：horizontal_fan_in(:809)/vertical_fan_out(:841)/vertical_fan_in(:873)。**horizontal_fan_out(:738) 按真实节点 id 分组功能正常**（同文件两种写法并存）。
- ref merman routing.ts:426,468,488,509 按 `edge.from`/`edge.to` 分组。修法：三臂改用 diagram.edges[i].from/to。

### G-177 ⚠️ TUI Markdown 表格降级为 raw 文本（tui）
- oc-tui markdown.rs:211-304 渲染臂仅 Text/CodeBlock/Blockquote/HorizontalRule/List 五种；opentui-core 将 `Token::Table{raw}` 降级为 raw markdown 文本（opentui-core markdown.rs:501-517 自认 "TextTableRenderable is not yet ported"）→ 表格内容可见但无 grid 结构。ref index.tsx:2059/message-parts.tsx:188 用 `tableOptions{style:"grid",cellPaddingX:1}`。

### G-178 ❌ mermaid 开放边 `A --- B` 仍渲染箭头（tui）
- drawing.rs:262-273 `draw_routed_edge` 无条件画箭头（无 arrowhead 检查）；:480-482 虽对 `!edge.arrowhead` 补画线尾，但 style.rs:169-171 `merge_flowchart_cell` 的 `is_arrow_head(existing) → return existing` 使先画的箭头胜出、线尾被丢。ref drawing.ts:187-195 在 drawRoutedEdge 内先判 `edge.arrowhead !== false`。修法：draw_routed_edge 增加 arrowhead 参数。

### G-179 ⚠️ mermaid seam 无流式截断恢复，宽度未接（tui）
- markdown.rs:613-657 语法错误直接回退代码块；全仓 grep last_good/truncate-retry 零命中；ref merman markdown.ts:272,290-314 有 lastGood Map + 截断重试。
- 宽度 clamp：渲染层管线在（render/mod.rs:84-119 layout_max_width.min(context_width)）但 seam 传 `MermaidRenderOptions::default()`（markdown.rs:631，context_width:None）→ 实际宽度从未接入。

### G-180 ⚠️ codeme 沙箱文档化决策新增子项：Logs 捕获缺失 + guest fetch 暴露（codemode）
- 沙箱架构（自研解释器→Bun guest）已收编文档化决策（见上文收编节），但两条子项为该决策下**新增**未登记面：
- ① console 捕获缺失：host.js:168-171 将 console 重路由为 NDJSON log 通知，不进执行结果 Logs（:2364-2365 自认）；ref stdlib/console.ts:9,21 捕获入 ctx.logs → 结果里 console 输出对模型不可见。
- ② guest fetch 暴露超 ref 面：ref codemode **无任何 fetch 面**（grep 零命中）；rust guest 为裸 Bun fetch 无超时/大小上限 —— 网络出口是 ref 沙箱不存在的能力面。

### G-181 ❌ mini 四缺口：model 偏好死绑定 / /new 不可用 / SIGWINCH / 断线恢复（tui/mini）
- ① runtime.rs:178 `let _preferences = ModelPreferenceRepository::new(...)` 下划线死绑定，全文件仅此一处（G-135 修的是 set_favorite 写路径，死绑定仍在）→ mini 内无法切换模型/变体。
- ② /new 恒 "new sessions unavailable"（runtime.rs:490-496）；③ Lifecycle::resize 存在（lifecycle.rs:1139）但 mini 全目录零调用、无 SIGWINCH（主 TUI app.rs:9583 才有）→ resize 后 wrap 宽度错；④ :232-250 断线 500ms 盲重连同 client，无 reconnect client 替换、无 hydrate 重放（transport.rs:590-591 自认 "the live port boots once and never reconnects"）→ 断线窗口事件丢失。

### G-182 ❌ 消息内多文件 patch 只渲染第一个文件（tui）
- tool_detail.rs:283 `files.first()`；ref index.tsx:3333-3378 `<For each={files()}>` 每文件一张卡 → ApplyPatch 多文件变更只显示 1 个。

### G-183 ❌ Question 工具无专用展示臂（tui）
- tool_detail.rs:630-864 臂集无 "question"，走 generic `question [k=v,...]` 兜底（:839-863）；ref index.tsx:3424-3458 有专用问答块（answered → "# Questions" 问答对；pending → "Asking questions…"）。

### G-184 ❌ 内联 shell 输出无流式拉取（≠G-33）（tui）
- G-33 的 1s 轮询仅 DialogShellOutput 查看器（app.rs:4407-4412 + store.rs:15733-15748 测试）；消息内联臂（tool_detail.rs:210）仅静态 `state.output`。
- ref index.tsx:2778,2826-2889：内联臂 cursor 增量 `shell.output` 拉取 + 1s 轮询 + SHELL_DISPLAY_LIMIT=1MiB + "[earlier output omitted]"——rust 全仓零命中。

### G-185 ⚠️ prompt history 降级：纯字符串、无回灌（tui）
- composer/mod.rs:232-255 `Vec<String>` 环（上限 100≠ref 50），无 PromptInfo（files/pasted 恢复）；storage.rs:254-268,448-465 prompt-history.jsonl 仅启动 heal+重写，解析结果不回灌 composer（:458-463 仅日志）。（注：stash 全套已在位——见勘误 #3。）

### G-186 ❌ slash 命令表硬编码，不消费服务器 command.list（tui）
- autocomplete.rs:140-251 V2_SLASH/SLASH_COMMANDS/ARGUMENT_SLASHES 三张静态表；oc-tui 全目录 grep `command.list` 零命中；ref autocomplete.tsx:486-494 将服务端命令并入候选 → 插件/自定义命令永远不出现。与 G-127（@-file 搜索，已修）不同面。

### G-187 ❌ 工具输出图片（ToolImages）未渲染（tui）
- rows.rs:536-580 image_strip_row 唯一调用点在 user_message_row；tool_detail 全臂 `images: Vec::new()`、tool_rows.rs 零图片引用；ref index.tsx:2456-2523（SessionImages + inlineToolImages）。mini 侧 tool_image_commits（mini/image.rs:102）仅作 dedup 标记。

### G-188 ❌ 权限阻塞工具分区（partitionPending）未移植（tui）
- ref rows.ts:71 + grouping/session.ts:163-176：等待授权的工具在折叠组外独立展示并沉底；rust components/message 零匹配（model.rs:610 的 pending 是排队输入，另一概念）→ 权限等待工具的可见性丢失。

### G-189 ⚠️ DialogSelect 三缺：details 行 / locked / Tab action 焦点（tui）
- ① SelectOption 无 `details` 字段（dialog_select.rs:45-65；:484-485 自认 wave-2）；② 无 `locked` 冻结；③ dialog.rs:737-753 键位层无 footer-action 焦点循环（ref dialog-select.tsx:434-443 moveAction+Tab；details 行 :816-824）。G-131（section 跳转）已修（dialog.rs:751-752），不同面。

### G-190 ⚠️ PowerShell `separator_after` 反引号续行组未实现 → save 授权串失稳（core）
- parse.rs:201-217 docstring 写着 ``/^\s+(?:`(?:\r\n|\r|\n)\s*)?/`` 但实现只贪心消费空白；ref parse.ts:278-279 正则含续行组 → 含续行的 PS 命令两侧生成不同通配授权串，升级后既有授权失配（重复弹权限）。

### G-191 ❌ subagent / mcp-resource 工具 executor 绕过执行管道（core）
- subagent.rs:652-656、mcp_resource.rs:265-277 直接 `register_executor` 裸注册；engine/mod.rs:2200-2205/2378-2381 直接调 register。标准路径 registry.rs:136-165 PipelineExecutor（before hook :207-227 → decodeInput :229 → leaf → after hook :294-334 → normalize_content :107）。
- 后果：这两工具不吃 tool.execute.before/after hook、不 decodeInput、不 normalizeContent（ref subagent.ts:102 / mcp-resource.ts:17,49 经统一管道 tool.ts:150）。可见 bug：read_mcp_resource 纯文本资源 content=[]（TS 由 normalizeContent 回退 stringify）。修法：改走 register_builtin/PipelineExecutor 包装。

### G-192 ⚠️ websearch：disable 移除 hook 缺 + Form kind 元数据缺（core）
- ref websearch.ts:184-192 在 context/compaction/generate 三 hook 于 disabled 时删工具；rust websearch.rs 无任何 session hook（grep 零命中；engine/mod.rs:2455-2459 直接 register_builtin）→ 用户选 disable 后工具仍出现在请求列表。
- ref websearch.ts:78,111 `metadata:{kind:"websearch.provider"}`；rust 两处表单（websearch.rs:373 及 choose 表单 :436 附近）`metadata: None` → 依赖 kind 分发的客户端无法路由。

### G-193 ⚠️ 插件内建行为组 5 项缺失：plan/identity/verbosity/optimize/init+review（core）
- 全仓库零命中：plan 模式注入（ref plugin/plan.ts；engine/mod.rs:2656-2713 seed_builtin_agents 无 plan agent）、模型 identity 注入（ref identity.ts:16-21 "# Your Model"）、GPT verbosity 自动 low（oc-ai 无 textVerbosity 注入；cli_config 的 verbosity 是 v1 schema 非请求注入）、per-provider system-prompt 优化模板（ref optimize.ts:10-16：gpt/astra/kimi/meta/trinity/anthropic）、command initialize/review（ref command/ 模板）。
- （skill opencode+report **已在位**：engine/skills.rs:92-133 include_str 内嵌——见勘误 #4。）与 G-147/148（已修的 ctx.model/effect API）不同批。

### G-194 ⚠️ compaction `due()` 边界：checkpoint 无 measured 时继续估算（core）
- ref compaction.ts:242-251 `measured < findLastIndex(isCheckpoint)`——checkpoint 存在而 measured=-1 时 `-1 < idx` → return false（未到期）；rust runner/compaction.rs:916-941 `if let (Some,Some)` 才比较，measured=None 时继续 `estimate >= ceiling` → 刚完成 native 压缩、尚无新回复的会话可能重复触发压缩。

### G-195 ⚠️ native compaction 两偏差：webSocket:false + finish=error 不重试（core）
- rust compaction.rs:638-671 prepare 硬编码 `web_socket:false`（:660）；ref compaction.ts:335 native 传 `"session"`（:569-590）→ rust native 压缩不走 session WS 缝（model_request.rs:914 仅 true 才生成 WebSocketSeams）。
- finish reason=error：ref compaction.ts:545-547 → `AIError(UnknownProviderError)` 可重试；rust compaction.rs:845-852 `FinishReason::Error => DeliverError::Failure`（Failure=永不重试）。

### G-196 ⚠️ LocationActivity 驱逐语义：无 sweep 播种、无中断后复查（core）
- rust location_services.rs:393-410 sweeper 对 expired 直接 interrupt+invalidate+remove；ref location-activity.ts sweep 先对 RcMap 缓存位置重新 touch（缓存位置不因空闲过期），中断 owners 后复查 execution.active，有新占用则 touch 保留不 invalidate。rust 有等价 bus touch（:364-380）但缺这两条 → 长空闲但 live 的位置被误驱逐、或驱逐刚接管的会话位置。

### G-197 ⚠️ oc-client：~49 个方法缺 404 declared + 全部方法无 per-request RequestOptions（client）
- 按名称对齐 ref promise/generated/client.ts（96 方法，83 个 404 declared）vs methods.gen.rs（142 op，75 个 404）：rust 缺 ~49 个 404 声明（agent.list、config.*、file.*、mcp.*、顶层 permission.*、plugin.*、project.list、provider.list、pty.create/list、session.export/import/skill/wait 等；部分已以别名路径声明如 fs.*↔file.*），另有 ~41 处多声明（命名迁移重合）。404 变 UnexpectedStatus，结构化 LocationNotFoundError 丢失。
- ref client.ts:277-282 `{signal,headers,onActivity}` 每方法尾参透传；rust 全部 142 方法签名仅 `&self, input`，grep RequestOptions/signal/on_activity 零命中。

### G-198 ⚠️ git/vcs：clone 无 --depth、快照 diff 无二进制检测/10MB 截断（core）
- repository_cache.rs:242-250 `clone <remote> [--branch] <path>` 无 `--depth 100`（ref git.ts:246-258 `--depth (depth ?? 100)`）→ 全量克隆。
- snapshot.rs:416-461 diff_blocking（git2）无二进制检测与 MAX_TOTAL_PATCH_BYTES=10MB 截断（ref git.ts:522-573 + vcs/patch.ts:8）；工作区 diff（oc-server/handlers/project.rs:1298-1300,1450-1535）已有 10MB cap 但同样缺 numstat "-"→patch 置空。与 G-30（git2 bounded）同域互补；BranchUpdated 事件已有（勘误 #5）。

### G-199 ⚠️ `~/.claude/skills` / `~/.agents/skills` 兼容 skill 根未并入 skills 扫描（core）
- rust config/discovery.rs:14,44-48 收集 .claude/.agents 根仅用于 instructions/config；skills 加载（engine/skills.rs:300-355）只扫 config skill/skills 目录+URL 源，未加 compat roots 的 `/skills`；ref compatibility.ts:39-56 明确扫描 → 从 Claude/Agents 迁移用户丢 skill。

### G-200 ⚠️ TUI：session.viewed 无服务端确认 + 渲染器 terminalHandoff 缺（tui）
- ref tui/context/session-tabs.tsx:209-242 view POST + 指数退避重试；rust oc-tui 仅从 session.list 的 `idle > viewed` 本地推导（data.rs:142-206，:196 自认 refinement 未做）→ 长离线 unread 陈旧。
- ref cli/default.ts:143 + tui/app.tsx:193,218,229-236 渲染器移交（preflight→terminalHandoff）；rust handlers/misc.rs:79-80 直接 `oc_tui::app::run` 自建渲染器（G-152 的 onStart 钩子在，但无 handoff）。

### 🟢 低

### G-201 ⚠️ Vercel Gateway VERCEL_OIDC_TOKEN 回退失效（ai）
- vercel_gateway.rs:42 `AUTH_ENV=["AI_GATEWAY_API_KEY","VERCEL_OIDC_TOKEN"]` 全仓无生产消费；实际 env 链 = catalog `["AI_GATEWAY_API_KEY"]`（经 facade.rs:815 生效）→ 仅 OIDC token 回退丢失；facade.rs:813 注释自称含 OIDC 亦不实。G-81 残留。

### G-202 ⚠️ 裸 `$NAME` 扫描字符集缺 [A-Za-z]（core）
- scan.rs:2809 `"_0123456789@*#?$!-"` vs ref scan.ts:1126 含字母 → `$FOO` 走普通字符路径、`$` 记入 literal → 更易置 bound_dollar → 与 variableSink/boundOpen 组合产生 ref 不会有的 dynamic-execution 拒绝（fail-closed 方向，非放行）。

### G-203 🟢 ripgrep.find 服务面无 1:1 移植（core）
- ref ripgrep.ts:52-61,193-218（exclude 模式链 `--glob=!`、onEntry 流式）唯一消费方 filesystem/search.ts:57-100；rust tool/ripgrep_runner.rs 仅移植二进制获取。fs find 端点已有原生索引等价实现（oc-server/handlers/fs.rs:254-336 + SearchIndex + ScanExclusions，注释引 ref search.ts:72）→ 记录实现路径差异（原生索引 vs rg 流式）+服务层缺口。

### G-204 🟢 shell 工具 metadata 缺 signal/timeout 字段（core）
- tool/shell.rs:568-578 metadata 仅 status/truncated?/exit?/shellID?；ref shell.ts:83-93 + shell/result.ts:36-43 展开含 signal?/timeout?。rust ShellToolOutput 的 output 本身有这两字段，缺的只是 metadata → 依赖 metadata 渲染 shell 卡片的 UI 缺信息。

### G-205 🟢 toSessionError：缺 Tool.Error 嵌套解包 + Integration.AuthorizationError→provider.auth（core）
- runner/mod.rs:506-542 仅 AiError 分支属实，但其余映射多已分布式存在（tool.execution step.rs:669,323、permission.rejected step.rs:681、no-route drain.rs:564-570、AgentNotFound context.rs:1338-1348）。真缺：ref to-session-error.ts:64-71 嵌套 AIError 递归解包+空 message 回退、Integration.AuthorizationError→provider.auth（integration/types.rs:303 有类型，runner 无映射）。

### G-206 🟢 promptCacheKey 校验接受大写 hex（core）
- model_request.rs:793-802 `is_ascii_hexdigit` vs ref model-request.ts:278 正则仅小写。实际影响≈0：两侧 Identifier 生成器均为 `ses_`+26 字符（identifier.ts:3-26 / identifier.rs:15+），生成的 id 永不命中 64-hex 分支 → 分歧仅在人为构造串可见。

### G-207 🟢 publishAll 批内 created 时间戳共享 + ephemeral data 强校验 is_object（core）
- bus.rs:696 整批一次取时钟（ref bus.ts:531-560 每事件各自取）→ 跨实现混用数据时重放幂等比较可产生假性分歧；bus.rs:625-628（publishAll :686 同）对 ephemeral 事件新增 `!data.is_object()` 拒绝（ref 无形状校验）→ Rust 附加约束，可拒合法发布。

### G-208 🟢 CLI cli-upgrade flock 缺失（cli）
- ref updater.ts:131,399 `flock.acquire("cli-upgrade",15min)`；rust oc-cli grep flock 零命中 → 并发升级竞态无互斥。（本批 update 流其余声称与 G-99✅/G-156✅ 重叠、method 探测=G-17 有意裁剪，仅此点为新。）

### G-209 🟢 Modal 动态模型发现缺失（ai）
- ref core/src/modal/models.ts + plugin/provider/modal.ts（Modal API load/derive：price/limit/variants 模板合成）；rust 全仓库零命中（provider_catalog/plugin-host 均无）。

### G-210 🟢 theme generateSystem 系统主题生成缺失（tui/theme）
- ref 实际位于 packages/tui/src/theme/system.ts:11-60（调色板→系统主题派生）；rust oc-tui/theme.rs:349-366 system 槽回退 v2 opencode 文档——有 OSC 11 模式检测（theme.rs:10-13）但无调色板派生。

## 归类与收编（第十二轮，不另立项）

- **剪贴板 host 写臂** → 重复 G-39（ℹ️ 有意，仅 Osc52Sink：app.rs:7810、store.rs:5885-5893），维持不变。
- **ai experimental/（Evaluation/system-one）** → 归类 G-7（evaluation lane 有意不移植）。
- **session.diff 经 SSE 公开** → 记录性分类面差异，不立项：registry.rs:217-220 注册致 is_public=true（event_manifest.rs:70），但全仓无发布点（仅 HTTP 端点 handlers/session.rs:1366；ref 同样从不发布）→ SSE 实际不转发。若未来开始发布 session.diff，需先并入 PRIVATE_TYPES。
- **插件 aisdk.language 句柄不可替换 / TUI 插件 API 收窄** → 已由 G-2 备注与 G-28 文档化决策覆盖；本轮新证据（plugin_apply.rs:2155）与之一致。
- **`aisdk:` npm 动态安装桥 / 动态 npm provider 工厂** → 归类 G-17 有意省略族（ref sdk-factory 运行时 npm 安装；rust typed error）。

## 勘误（第十二轮经核验撤销/修正的初报，勿再列入待修）

1. **bus log() 有界 64 静默断流** — 不属实：bus.rs:1108 channel(64) 但 `tx.send().await` 为背压等待；send 失败仅当 receiver 丢弃（消费者主动关闭）=预期取消语义；wake 读前 re-arm + cursor 基 read_through（:1114-1121,1165-1189），慢消费者不丢事件。与 ref 差异仅在缓冲策略。
2. **无状态 Generate.text 服务缺失** — 不属实：oc-server/handlers/misc.rs:42-114 已完整实现（POST /api/experimental/generate，routes.gen.rs:288-299；prompt 校验、ModelSelection/Unavailable 映射 :69-91、x-opencode-session 头 :96-100、503 UnavailableError）；差异仅为架构位置（server handler 内联 vs ref core 服务层）。
3. **prompt stash 全套缺失** — 撤销：stash 已全套在位（storage.rs:236-309、store.rs:9555-9658 stash_init 完整恢复 chips、:9757 push、:9789 pop、command_palette.rs:308-352 三命令、app.rs:7693 boot 接线）。仅 history 半边成立（→G-185）。
4. **内建 skill opencode+report 缺失** — 撤销：engine/skills.rs:92-133 include_str 内嵌已注册。
5. **VCS 无 BranchUpdated 事件流** — 撤销：`vcs.branch.updated` 已有（announce.rs:532、sse.rs:719、tests/be1_lane.rs:303）。
6. **workspace 驱动注册表"仅 DB 表"** — 撤销：environment/workspace.rs 有 Registry.register(:82-94)+local driver+create/provision/connect/destroy(:145-237)；deferred 仅 idle janitor/共享 provision/连接池（模块头 :3-10 注释，非 local provider 时才 load-bearing）。
7. **Vercel Gateway env 链为空** — 修正：AI_GATEWAY_API_KEY 经 catalog environment 生效（facade.rs:815）；仅 VERCEL_OIDC_TOKEN 丢失（→G-201）。
8. **session.diff SSE 泄露** — 降级：无发布点，实际暴露面与 ref 一致（见归类节）。
9. **codemode "ref fetch 30s 超时封装未移植"** — 撤销该出处：ref codemode 无 fetch 面（grep 零命中）；rust 裸 Bun fetch 属暴露面超 ref（→G-180②）。
10. **oc-client 404 declared 缺 ~30** — 修正为 ~49（按名称对齐后实测；另 ~41 处多声明为命名迁移重合）。
