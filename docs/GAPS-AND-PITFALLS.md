# 移植缺口与坑（Gaps & Pitfalls）

> 2026-10-30 二次复核版；同日 burndown 完成状态更新（gb2-burndown，commits a193f4b..phase-6）。每条均经两侧源码取证（rust `/opt/opencode-rust` vs ref `/opt/opencode-ref`），未采信仓库内既有评审文档。总报告见 `COMPREHENSIVE_PORT_AUDIT.md`。
>
> 状态图例：✅ 已解决（本轮 burndown 落地）· ❌ 缺失 · ⚠️ 部分/偏差 · ℹ️ 有意决策（已文档化）· 🧹 清理项

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
- ref 死代码：`mcp/resolve.ts`、`plugin/inventory.ts`、`debug/redact.ts`（ref 自身未注册）；ref 的 `ignore` 依赖在其源码中也无 import。
- Web/GUI/云生态：web/desktop/enterprise/console/app/posts/storybook/identity/containers/function 等整包不移植（codemode/latex/merman/simulation 的**功能本体**已在 oc-core/oc-tui 内实现或按上文明示处理）。
- rust-extra 超出 ref 表面的部分（`upgrade -m local-file|github`、completion、`--force`、run 的 TTY 权限提示、serve hostname 解析报错、`$PWD` 陈旧值守卫等）均为防御性收紧且有注释。

## 本轮 burndown 之外的已知残留（后续工作）
- G-1 consumer 迁移：tools/handlers（read/write/edit/shell/glob/grep/patch、mcp/stdio、pty）尚未切换到 Environment Files 服务（服务本体与接线已就绪）。
- G-4 插件注册自定义 worktree strategy（ref plugin editor 面）。
- G-6 Shell 输出查看器 1s 轮询（现一次性拉取 + 64KiB tail）。
- G-8 https/socks 代理 URL 支持（TLS-to-proxy / SOCKS 握手）。
