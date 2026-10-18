# 08 · 插件平台子系统 parity 扫描（插件宿主 / MCP / 集成 / 权限 / 表单 / hooks）

> 基线：opencode v2.0.24（`/opt/opencode-ref`，只读）↔ `/opt/opencode-rust`（workspace 2.0.24）。
> 方法：逐文件通读两侧子系统源码（见 §6 覆盖清单），以 v2 语义为 EXPECTED、Rust 代码为 ACTUAL 做行为级比对；对每个「疑似缺失」结论在全仓 grep 反查防误判。只读审计，仅输出本报告。
> 严重度：S0 = 阻断核心功能/数据损坏；S1 = 重要能力缺口（插件生态/远程服务器可用性受损）；S2 = 次要/边缘差异。
> 状态：`OK`（对齐）/ `MISSING`（无实现）/ `STUB`（占位返回）/ `PARTIAL`（部分语义）/ `WRONG`（行为相反或错位）/ `EXTRA`（Rust 特有增量）。
> KNOWN/NEW：与 `ROADMAP.md`、根 `FINDINGS.md`、`docs/audit{A..F}-*.md`、`docs/parity-scan-2026-10-18/*` 交叉标注。

---

## 1. 总览

**架构根差异（一切细项的上游）**：v2 的插件运行时是**进程内 Effect 插件系统**——`Plugin.Service` 按 location 持有注册表（revision 前缀 diff 激活，`core/src/plugin.ts:89-180`），内置 50+ 内部插件（`plugin/internal.ts:213-271`，pre/post 两段）与 SDK/config 包插件统一走同一条 `ctx`（Effect Context，`plugin/host.ts:115-570`）。Rust 移植采用**双轨**：

1. **内建能力 native 化**——v2 的内部插件（工具、agent 种子、config watcher、optimize/warming 等）不再以插件形态存在，直接实现在 `oc-core`（engine/hooks/各 lane）；
2. **用户 JS 插件外置子进程**——`oc-plugin-host` 以受监督的 `bun host.js` 子进程承载（NDJSON JSON-RPC 双向 stdio），注册期同步快照（`registrations.rs:207-225`）+ 运行时有限 RPC（load/unload/dispatch/command.execute/tool.execute/hook.dispatch/codemode.evaluate/slot.*）。

这一分歧是 MIGRATION/ROADMAP 的既定裁决（ROADMAP D-03 全记录），**本身记为已裁定（documented divergence）**；本报告关注的是：在该架构下，v2 对插件作者承诺的**API 面、hook 语义、KV、RPC、MCP/集成/权限/表单行为**有多少真实落地、多少在生产路径断线。

**评分卡**（粗粒度）：

| 子系统 | v2 规模 | Rust 规模 | 判定 |
|---|---|---|---|
| hooks 注册表本体（18 点位） | hooks.ts 108 L + effect 域 | hooks.rs 2071 L | **OK**（native 侧语义齐；guest 侧接线断，见 D-21） |
| 插件宿主（进程/协议/监督） | in-process | oc-plugin-host 5145 L | **OK+EXTRA**（崩溃归因/退避/打击数是增量） |
| 插件 API 面（ctx.*） | host.ts 456 L（20 域） | host.js makeCtx ~420 L（12 域） | **PARTIAL**（8 个域缺失，session 域 15→5 接线） |
| 插件来源/发现/安装 | source.ts+source.bun+config/plugin/source | resolve.rs 428 L + install.rs 481 L + CLI | **PARTIAL**（目录自动发现缺失） |
| 插件 KV storage | KV→DB（持久） | sandbox tmp JSON 文件 | **WRONG**（进程级易失） |
| 插件 RPC（ctx.rpc + /api/rpc） | rpc.ts 289 L | traits + 占位 handler | **STUB** |
| MCP client/registry | mcp/ 2103 L | mcp/ 3810 L（不含测试） | **OK 主干 / PARTIAL**（OAuth 流、protocol pin、credential 重连缺失） |
| MCP elicitation | client.ts 内嵌 | elicitation.rs 503 L | **OK** |
| 集成 host | integration.ts 834 L | integration/ 5948 L | **OK 主干 / PARTIAL**（内置 provider 集合差 4 项） |
| 权限引擎 | permission.ts 347 L | permission.rs 1493 L + saved/sql | **OK**（hook override 未接线） |
| 表单 | form.ts 370 L | forms.rs 963 L | **OK** |

---

## 2. 架构映射

| v2 概念 | v2 位置 | Rust 对应 | 判定 |
|---|---|---|---|
| Plugin.Service（激活/前缀 diff/失败降级恢复） | `core/src/plugin.ts:43-257` | 无对应（进程级一次 boot） | 架构性缺失（见 D-07/D-02） |
| PluginHost.make（插件 ctx 装配） | `core/src/plugin/host.ts:47-572` | `oc-plugin-host/src/host/host.js:400-820`（makeCtx）+ supervisor 回答端 | 重设计（子进程协议） |
| PluginHooks（register/trigger/has） | `core/src/plugin/hooks.ts:63-106` | `oc-core/src/hooks.rs:738-896`（HookRegistry，18 点位宏） | OK（native）；guest 断线 D-21 |
| 内部插件清单 pre/post + guarded | `core/src/plugin/internal.ts:213-271` | `oc-core/src/hooks.rs:1353-1513`（InternalPlugin/PluginSupervisor，host_slot 占位） | PARTIAL（D-21/D-04） |
| 插件监督（watcher/24h/outdated/激活代） | `core/src/plugin/supervisor.ts:116-260` | 子进程 supervise_loop（重启退避+归因打击）`oc-plugin-host/src/supervisor.rs:1414-1500` | 热重载缺失（D-02/D-03） |
| 插件 SDK（effect + promise 双 API） | `packages/plugin/src/**`（promise/ 24 文件 + effect/ 22 文件） | guest 内嵌 ctx（host.js）| 面收窄（§3.3） |
| 插件源（本地/npm/git/discovery） | `packages/plugin/src/source*.ts` + `core/src/config/plugin/source.ts` + `plugin/source-directory.ts` | `oc-plugin-host/src/resolve.rs` + `context/install.rs` + `oc-cli/src/handlers/plugin.rs` | PARTIAL（D-01） |
| host→guest RPC 协议 | （无进程边界） | `oc-plugin-host/src/protocol.rs`（NDJSON JSON-RPC，id 空间 1↔2^40 分离） | EXTRA（新协议，死锁规则四条成文） |
| 插件 KV | `core/src/plugin/host.ts:602-629`（KV.Service→DB） | `oc-plugin-host/src/supervisor.rs:1560-1577`（`<sandbox>/plugin-storage/<id>.json`） | WRONG（D-18） |
| 插件 RPC | `core/src/rpc.ts` + `@opencode/schema/rpc` | `oc-core/src/hooks.rs:1272-1313`（RpcDefinition/RpcHandler/RpcPort trait，TODO 注释）+ `oc-server/src/handlers/misc.rs:180-188` | STUB（D-14） |
| MCP 客户端 | `core/src/mcp/client.ts`（SDK StreamableHTTP/stdio） | `oc-core/src/mcp/client.rs`（rmcp + reqwest/tokio 子进程） | OK 主干 |
| MCP 注册表 | `core/src/mcp/index.ts` | `oc-core/src/mcp/registry.rs` | OK 主干 |
| MCP OAuth | `core/src/mcp/oauth.ts`（532 L） | bearer 查询 seam（`registry.rs:181-253`） | MISSING（D-30） |
| 集成 host | `core/src/integration.ts` + `integration/connection.ts` | `oc-core/src/integration/{mod,types,flows,credential,console,providers*,wellknown_config,vertex,copilot}.rs` | OK 主干 |
| 权限引擎 | `core/src/permission.ts` + `permission/{saved,sql}.ts` | `oc-core/src/permission.rs` + `permission/{saved,sql}.rs` | OK |
| 表单 | `core/src/form.ts` | `oc-core/src/forms.rs` | OK |
| oauth 回调页 | `core/src/oauth/page.ts` | `integration/flows.rs:204-232`（bootstrap/success 页） | OK（并入 flows） |
| Code Mode 默认表 | `core/src/plugin/mcp-codemode-defaults.ts` | `oc-plugin-host/src/context/codemode_defaults.rs`（+ registry `apply_codemode_defaults`） | OK（规则逐条对齐） |

---

## 3. 能力矩阵

### 3.1 插件来源、发现与安装

| 能力 | v2 | Rust | 状态 |
|---|---|---|---|
| 配置 `plugins[]`（字符串 / `{package, options}` / v1 `plugin` 迁移） | `config/plugin/source.ts:113-120` | `oc-plugin-host/src/resolve.rs:43-69`（parse_ops；v1 迁移在 `oc-core/src/config/normalize.rs:436-452`） | OK |
| remove 指令（`-id`/`-*`，后写覆盖先发现） | `config/plugin/source.ts:117-119` + supervisor 归并 | `resolve.rs:127-165`（先归约后解析） | OK |
| **`plugin`/`plugins` 目录自动发现**（config roots 下每 entry 的 `plugin|plugins` 子目录里的 .ts/.js/目录/符号链接） | `core/src/plugin/source-directory.ts`（names）+ `config/plugin/source.ts:122-133` | **无**：`oc-server/src/plugins.rs:294-314` 与 `oc-tui/src/plugin_boot.rs:150-172` 只读文档 `plugins[]` 数组 | **MISSING（D-01, S1, NEW）** |
| 本地路径解析（`./`/`../`/绝对/`file://`；目录经 package.json exports `./server`→`.`→main→index 探测） | `config/plugin/source.ts:139-167` + `packages/plugin/src/host.ts:17-40` | `resolve.rs`（同序探测，`resolve_entry` 于 install.rs） | OK |
| npm 包规格（`pkg`/`pkg@ver`/scoped/git 规格）安装缓存解析（离线优先，未装=失败记录） | supervisor 两段 resolve（本地可用先激活，pending 再等安装，`supervisor.ts:154-173`） | `resolve.rs:127-160`（`<cache>/opencode/npm/<pkg>@<ver>/<ts>/node_modules/<pkg>` 最新时间戳胜出；离线失败记录） | OK（安装动作在 CLI `plugin add`，`oc-cli/src/handlers/plugin.rs:125-176`） |
| 入口三元组 server/tui/rpc（Host.resolve） | `packages/plugin/src/host.ts:17-40` | server 走 resolve.rs；tui 走 `load.tui`（host.js:1546+）；**rpc 入口无** | PARTIAL（rpc 依赖 D-14） |
| guarded id（`opencode`、`opencode.policy` 不可被 remove 移除） | `internal.ts:269-271` | `oc-core/src/hooks.rs:1341-1344` 定义 `is_reserved_plugin_id`（"opencode." 前缀）**但无生产调用方**；`resolve()` 的 remove 不查 guarded | PARTIAL（D-04, S2, NEW） |
| `plugin add/remove/update` CLI + 安装缓存 | v2 CLI（npm 安装） | `oc-cli/src/handlers/plugin.rs`（pinned bun 安装、lockfile 冻结、JSONC 写回 D-01 修复） | OK |
| 运行时 outdated/updating 跟踪 | `plugin/update.ts`（24h npm check + changes 流），supervisor 注入 `Plugin.Source.outdated/updating` | 无运行时跟踪；`/api/plugin/check|update` 为 STUB（misc.rs:152-174）；`PluginInfo.outdated/updating` 恒 `None`（plugins.rs:182-184） | **STUB（D-03, S2, KNOWN—auditE §1.4 🟡）** |

### 3.2 加载顺序、生命周期、隔离与监督

| 能力 | v2 | Rust | 状态 |
|---|---|---|---|
| 内置 pre →（sdk/instance/config 包）→ 内置 post 顺序 + 重复 id 首见胜出+报失败 | `internal.ts:213-267` + supervisor.ts:138-152 | `oc-core/src/hooks.rs:1496-1513`（PluginSupervisor::boot，pre→host_slot(())→post；dedup+containment 齐备） | OK（顺序容器在）；**host 槽空转（D-21）** |
| per-location 激活（每 location 一套 Plugin 注册表） | `plugin.ts:17-257`（LocationGraph 节点） | 进程级单一 host；`plugin_list` handler 自述 documented divergence（`oc-server/src/handlers/misc.rs:141-146`） | PARTIAL（D-07, S1, KNOWN—代码内自述） |
| revision 前缀 diff（未变前缀不重载；失败 revision 不重试直到变更；旧版 fallback 恢复） | `plugin.ts:100-180` | 无（重载=整插件 unload/load） | 架构性缺失（影响小，S2） |
| 文件 watcher 热重载（config 变更 + 模块 import 图 digest + 24h 周期 + Plugin.Event.Updated） | `supervisor.ts:196-213`；digest 图 `packages/plugin/src/source.bun.ts:20-80` | digest 扫描本身已移植（host.js:1234-1262 `scanDigests`，Bun.Transpiler.scanImports + CJS 缓存驱逐 + `?t=` generation cache-bust），但**只有显式 `reload_now()` 触发，且无生产调用方**（全仓 grep 仅 oc-plugin-host 内部/测试） | PARTIAL（D-02, S2, NEW） |
| 插件失败隔离（一个插件 setup 失败不影响其余；inventory 报 failed） | `plugin.ts:146-170` | guest 端 per-plugin try/catch + supervisor 归因；`PluginStatus::Failed` 进快照（registrations.rs） | OK |
| 崩溃监督/退避/打击禁用（3 strikes 禁插件、8 次连续重启放弃子系统） | （无进程边界，N/A） | `oc-plugin-host/src/supervisor.rs:473-530,1414-1500`；backoff 100ms→4s | **EXTRA**（隔离带来的增量，正面） |
| bun 缺失降级（单条日志 + 全 no-op）+ 托管 bun bootstrap（pinned 1.3.10 sha256） | N/A（v2 宿主即 bun/node） | `lib.rs:224-233,371-388` + `oc_util::bunman` | EXTRA |
| 插件卸载（dispose 回调 + 订阅/命令清理） | Scope.close 级联 | host.js:879-900 handleUnload（dispose + tui disposers + dialog/sidebar 表清理） | OK |
| v1 插件形态（`server()` 导出） | v2 有 v1 兼容层（`core/src/v1`） | guest classify 识别 v1 但仅记日志「tolerated (ignored)」（host.js:853-857） | PARTIAL（D-05, S2, NEW） |
| Effect 契约插件（`define({id, effect})`） | 原生执行 | 「loaded inert」（host.js:859-866） | 架构性（KNOWN） |

### 3.3 插件 API 全表面（guest `ctx.*`）

v2 Context 域全集（`core/src/plugin/host.ts:115-570`）vs Rust guest ctx（`oc-plugin-host/src/host/host.js:400-820`）+ supervisor 回答端（`supervisor.rs:265-460`）：

| ctx 域 / 方法 | v2 (host.ts) | Rust (host.js / supervisor.rs) | 状态 |
|---|---|---|---|
| `plugin.id` / `app.version` / `directory` / `location` / `options`（`{package,options}` 配置项） | :115-119 | host.js:424-432（options 经 `entry_options` 注入，resolve.rs:109-120） | OK |
| **agent**：transform(editor{list,get,default,update,remove})/reload/list/get | :120-156 | host.js:436-459（编辑器语义齐：update 缺省建 defaultAgent、default 写 agentDefault）；快照→`plugin_apply.rs:29-52` 落 AgentRegistry | OK |
| **command**：add/transform(draft.add)/reload/list | :250-254 | host.js:485-523；`command.add`→native 注册（supervisor.rs:547-571）+ `/api/command` 合并（agent.rs:55-79，CD-12/CD-34 已接） | OK |
| **skill**：transform(add/update/remove)/reload/list | :444-457 | host.js:526-546（编辑器齐）→ 快照 `skills` **但引擎 SkillTable（config 发现）不消费**（`engine/mod.rs:753-754`；plugin_apply.rs 无 skills 步） | **PARTIAL（D-35, S1, NEW）** |
| **tool**：transform(editor{list,get,namespace,add,update,remove})/reload/list/hook | :462-467 + promise/tool.ts:27-40 | host.js:549-570（编辑器**缺 `namespace()`**，有 add/update/remove）；`tool.execute` 执行分派（lib.rs:524-558 + plugins.rs:483-506 经 ToolRegistry executor） | PARTIAL（D-16, S2, NEW） |
| **mcp**：transform(list/get/set/update/remove)/reload/list（可跨 location） | :374-403 | host.js:462-482（编辑器齐，seed=config+codemode 默认表）；快照 `mcps` **不进 McpRegistry**（plugins.rs:247-258 只读 config 文档） | **PARTIAL（D-33, S1, NEW）** |
| **session**：15 方法（create,get,remove,switchAgent,switchModel,prompt,generate,command,compact,synthetic,interrupt,update,move,wait,context）+ hook | :527-569 + promise/session.ts:154-170 | guest 暴露 11（get,interrupt,context,messages,list,prompt,synthetic,update,remove,wait,switchModel）；**端口仅接线 5**：prompt/get/list/messages/interrupt（plugins.rs:525-584，其余返回 "not wired … yet"）；create/switchAgent/generate/session.command/compact/move 未暴露；`messages`/`list` 为 v2 插件 API 所无的 EXTRA | **PARTIAL（D-10, S1, KNOWN—代码内自述 TODO）** |
| **generate**：text | :271-273 | host.js:591-596 → `generate.text` 经 session port | OK |
| **permission**：hook(evaluate)/list/get/reply | :404-427 + promise/permission.ts:20-24 | guest 仅 `permission.reply`，且 supervisor 直接回 null（**STUB**，supervisor.rs:307-311）；hook/list/get 无 | **STUB（D-11, S1, NEW）** |
| **storage**：get/set/remove/scan（prefix/after/limit；KV→DB 持久） | :458,602-629 | host.js:606-626 + supervisor.rs:643-680（scan limit clamp 1..500 为 EXTRA 约束）；**落盘在 PID 唯一 sandbox `/tmp/opencode/oc-plugin-host-<pid>-…`（lib.rs:813-821）→ 每次进程重启清零** | **WRONG（D-18, S1, NEW）** |
| **event**：subscribe()（AsyncIterable；过滤 ServerEvent \| `rpc.*`）/subscribe(types,handler) | :255-264 | host.js:639-724（双形态齐，512 深背压丢弃 EXTRA）；**服务器 embedder 未接 tap**——`PluginBoot` 不向 `host.tap()` 发布任何事件（oc-server 无 `tap()` 调用），只有 TUI embedder 经 store tap（plugin_boot.rs:253）送 TUI 总线事件；`rpc.*` 事件两侧均无来源 | **PARTIAL（D-13, S1, NEW）** |
| **rpc**：client(register/definition)/register+events.emit | :119 + rpc.ts:69-175 | **无 ctx.rpc**；`/api/rpc/:rpcID/:method` 恒 `rpc.internal`（misc.rs:180-188）；oc-core 侧仅 trait 面（hooks.rs:1272-1313，`TODO(plugin-lane)` 注释） | **MISSING/STUB（D-14, S1, NEW）** |
| **aisdk**：hook("sdk"/"language") | :157-188 | 无 | MISSING（**永久分歧 R1**，KNOWN—ROADMAP D-03/FINDINGS §5 R1：JS 对象不可跨进程，等价能力经 `session.model.request`） |
| **provider**：list/get/reload/transform(add/update/remove/models.*) | :189-226 | 无（provider 域 native 化） | MISSING（架构替代，KNOWN—auditE §1.8） |
| **model**：list/default/reload/transform | :227-249 | 无 | 同上 |
| **integration**：list/get/connect.key/oauth.{connect,status,complete,cancel}/command.*/reload/connection.*/transform | :274-373 | 无 ctx.integration；内置集成 host 在 core（§3.8）——插件不可注册集成方法 | MISSING（S2，架构替代主体在；影响：第三方 OAuth 集成插件不可写） |
| **reference**：list/reload/transform(add/remove) | :431-443 | 无 | MISSING（S2） |
| **vcs**：get/base/branch.list/status/diff/transform/reload | :468-478 | 无 | MISSING（S2，KNOWN—auditE §1.7） |
| **websearch**：providers/query/reload/transform | :479-507 | 无 | MISSING（S2，KNOWN—auditE §1.7 websearch 未接线） |
| **worktree**：list/create/refresh/remove/reload/transform | :508-526 | 无 | MISSING（S2） |
| **experimental.terminal.read** | :266-270 | 无 | MISSING（S2） |
| **shell.hook**("create.before") | :459-461 | 无 ctx.shell | MISSING（D-15 关联，S2） |
| **ping / log** | （隐式） | host.js:730-731 → supervisor log 门（warn/error 落 stderr） | OK/EXTRA |
| **ui**（toast/sidebar/footer/dialog/grouping/home） | TUI feature-plugin 面（packages/plugin/src/tui + tui 插件） | host.js:739-819 + ui.rs + supervisor.rs:328-360（注册期 native 覆盖 + 运行期请求队列） | 对应 v2 TUI 插件面（细节归 06-tui.md / auditB；本报告不重复） |
| **theme.register / slots.register / tui(api) 桥 / keymap/router（TUI 面）** | packages/plugin/src/tui/*（560 L context） | host.js:1264-1754（@opentui/solid shim、树扁平化、route/theme/layout 推送、slot.click） | PARTIAL——为旗舰插件定制的子集（route.register/keymap/mode/attention/kv 为 no-op stub，host.js:1625-1645）；KNOWN（SCAN-AB/RESCAN 系列） |
| **hostFlavor** | （无此概念） | host.js:428 `"v2"` | EXTRA |

### 3.4 hooks：点位、顺序、返回值、短路、异常

**点位矩阵**（v2 `plugin/hooks.ts:13-19` Domains ↔ `oc-core/src/hooks.rs:859-896`）：

| 点位 | 注册 | 引擎触发点（v2 → Rust） | 状态 |
|---|---|---|---|
| session.prompt | ✅ | `session/prompt.ts:40` → `oc-core/src/location_services.rs:643` | OK |
| session.context | ✅ | `model-request.ts:404-407` → `session/runner/model_request.rs:546` | OK |
| session.compaction | ✅ | 同上 :405 → `model_request.rs:537`（+ compaction.rs:585 注释自证 after-request-hooks） | OK |
| session.generate | ✅ | `model-request.ts:410`（独立 trigger）→ **Rust 无独立触发**：`model_request.rs:546` 的 `_ =>` 把 Generate 归并为 context 事件 | **WRONG（D-22, S2, NEW）** |
| session.title | ✅ | :411 → `model_request.rs:518` | OK |
| session.model.request | ✅ | :288 → `model_request.rs:626` | OK |
| session.http.request / http.response | ✅ | :326/:338（fetch wrap 内）→ **typed（hooks.rs:476-494）但无 dispatch 站点** | **MISSING 接线（D-23, S2, KNOWN—FINDINGS D-03 表 (a) 类）** |
| session.experimental.ws.{handshake,send,receive} | ✅ | ref 同 model-request 内 → typed（hooks.rs:496-525）无 dispatch | 同上（D-23） |
| session.retry | ✅ | `runner/retry.ts:78-88` → `model_request.rs:691`（写回 `*event = shaped`，D-03 修复项） | OK |
| tool.execute.before（唯一可失败点位：Tool.Error 拒绝） | ✅ | `tool.ts:104` → `tool/registry.rs:342`（`HookError::ToolRejected` 短路） | OK |
| tool.execute.after（error 腿重写 error） | ✅ | `tool.ts:132-149` → `registry.rs:435,495` 双腿应用 | OK |
| permission.evaluate | ✅ | `permission.ts:179` → Rust **两处断线**：`Permission::new(..., hook: None)`（`engine/mod.rs:705-720`）且 `HookRegistry::dispatch_permission_evaluate` 无生产调用（HookOverride seam 在 `permission.rs:244-245,439-457` 空挂） | **MISSING 接线（D-24, S2, KNOWN—FINDINGS「production dispatch site unwired」）** |
| shell.create.before | ✅ | `shell.ts:274` → `oc-core/src/shell.rs:432-454` 有 `before: Option<BeforeHook>` 参数但生产调用传 None；`dispatch_shell_create_before` 无调用 | MISSING 接线（D-15, S2, NEW） |
| aisdk.sdk / aisdk.language | ✅ | host.ts:157-188 → typed（hooks.rs:602-620）但 JS 对象不可跨界 | **永久分歧（KNOWN—FINDINGS §5 R1）** |

**语义细节**（D-03 spike 已定案，核验属实）：

| 语义 | v2 | Rust | 状态 |
|---|---|---|---|
| 注册序分发 + `&mut` 链 | hooks.ts:88-95 | hooks.rs:694-721（handler 列表拷贝后顺序 await；`&mut event` 逐个传递） | OK |
| provider 过滤 per-entry 重读 live 事件 | hooks.ts:90 | hooks.rs:705-717（循环内重读 `hook_provider()`；测试 :1614-1678） | OK |
| handler 失败 → 分发失败、后续跳过 | yield* 传播 | `handler.call(&mut event).await?`（hooks.rs:718）+ guest 端首个失败拒绝 RPC（host.js:1203-1210，`corpus_hook_failure_fails_the_dispatch`） | OK |
| 子进程 copy-in/copy-out（对可观察行为等价） | by-reference | FINDINGS.md 全文论证 + e2e 证明 | 已裁定（KNOWN） |
| `Registration.dispose` + Scope 级联 | Scope.addFinalizer | `hooks.rs:98-145`（Drop/dispose 同步）+ HookScope | OK |
| `has(domain,name,providerID)` | hooks.ts:97-102 | 每点位 `has_*`（hooks.rs:725-732） | OK |
| **guest 插件 hook → 引擎接线** | （同进程，天然） | **断线**：guest `session.hook/tool.hook` → `hook.register`（supervisor.rs:279-296）→ 快照 `hooks`（registrations.rs:120-126）+ `Bridge::dispatch_hook/hook_subscribers`（context/mod.rs:112-150，仅测试使用）；**`PluginSupervisor.host_slot: ()`（hooks.rs:1423,1506-1508）占位，生产无任何 dispatch 走 guest** | **MISSING 接线（D-21, S1, KNOWN—FINDINGS §5；注意 ROADMAP D-03 行尾声称 "wired in the engine lane" 与代码不符，见 §5）** |

### 3.5 插件 KV（storage）

见 D-18：API 面（get/set/remove/scan + prefix/after/limit + 命名空间按 pluginID 十六进制转义）在 v2 `host.ts:602-629` 以 `plugin:<hex(id)>:` 前缀写入 KV.Service（**DB 持久**）；Rust 在 supervisor 内实现同形 API（supervisor.rs:640-680）但存储为 `<sandbox>/plugin-storage/<sanitized>.json`，sandbox 是 `/tmp/opencode/oc-plugin-host-<pid>-<n>-<nanos>`（lib.rs:813-821，**每次进程启动新建**）→ 插件状态跨重启即失。oc-core 另有 `PluginStorage` trait（hooks.rs:1261-1266，`TODO(plugin-lane)` DB 后端）未与 supervisor 实现合流。

### 3.6 插件 RPC（guest-host RPC 协议）

- **host↔guest 传输协议（EXTRA，Rust 特有）**：NDJSON JSON-RPC（`protocol.rs`），请求/响应/通知三态、guest id 从 2^40 起、坏行跳过；死锁四规则成文（lib.rs:29-56）并有 storm/fuzz/timeouts 测试背书。这是新协议而非 v2 语义的移植，判 EXTRA（正向）。
- **v2 插件 RPC（`ctx.rpc`）**：register（definition+handlers，schema 校验/声明式错误/事件 emit `rpc.<id>.<event>`）、client（方法调用+事件订阅）、`Rpc.call`（多注册取最后、`rpc.unavailable/method_not_found/invalid_input/invalid_output/internal` 失败面，rpc.ts:69-175）。Rust：`ctx.rpc` 不存在；`/api/rpc/:rpcID/:method` 恒 500 `rpc.internal`（misc.rs:180-188）；oc-core trait 面 `RpcPort` 为 `TODO(plugin-lane)`（hooks.rs:1299-1313，location_services.rs:473-492 的实现仅测试桩）。事件侧 `rpc.*` 类型在 SSE 公开集中已预留（auditD §3.4）但无发布者。→ **STUB（D-14）**。

### 3.7 MCP

| 能力 | v2 | Rust | 状态 |
|---|---|---|---|
| 配置形态 local（command/cwd/environment）+ remote（url/headers/oauth/codemode/timeout{startup,catalog,execution}） | `@opencode/schema/mcp` + index.ts Options | `oc-schema/src/mcp`（ServerConfig::Local/Remote 同字段） | OK |
| stdio 传输（cwd 解析、env 合并、`opencode` 命令加 BUN_BE_BUN=1、进程组） | `mcp/stdio.ts` + client.ts:247-259 | `client.rs:817-850`（逐项对齐，含 process_group(0)） | OK |
| HTTP 传输（Streamable HTTP；`?codemode=false` 注入 + 400/404 一次原 URL 重试） | client.ts:260-292 | `client.rs:852-936`（同逻辑；瞬时重试 delays 250/500ms，v2 为 250ms→1s，S3 级差异） | OK |
| **legacy SSE-only 服务器** | SDK StreamableHTTPClientTransport 自带 SSE 回退 | rmcp `StreamableHttpClientTransport` 仅 streamable-http + `text/event-stream` 响应流；无旧式 `/sse` 端点传输 | **PARTIAL（D-31b, S2, NEW—需实测旧服务器）** |
| protocol 协商（`protocol: legacy/auto/pin` + UnsupportedProtocolVersionError 提示） | client.ts:184-199,384-388 | 无对应（rmcp 自带协商，无 pin/legacy 开关） | MISSING（D-31a, S2, NEW） |
| 超时三段（startup/catalog/execution）+ 默认值 | client.ts:216-218,317-318 | `client.rs:774-790`（DEFAULT_* 同值） | OK |
| 瞬时错误重试（指数退避、上限 2 次、deadline 内） | client.ts:170-182（250ms → 1s（×4 因子）共 2 次） | `client.rs:253-276`（retrying）+ connect 侧 delays 250/500ms | OK（重试节奏略异：250/500 vs 250/1000，S3 级） |
| 会话过期（legacy 404/400 识别 → SessionExpired → 前台 recovering / 后台 fork recover；一次重放） | client.ts:228-244 + index.ts:350-365 | `client.rs:109-118 is_session_expired` + `registry.rs:858-928`（replay_after_recovery 同序） | OK |
| tools/prompts/resources/templates/readResource/prompt/callTool（`_meta sessionID`、progress 保活） | client.ts:330-379 | `client.rs:479-745`（`call_tool` 带 `_meta["ai.opencode/sessionID"]`） | OK |
| list-changed 订阅（tools/prompts/resources → 刷新+发事件） | client.ts:156-161 + index.ts:402-425 | `client.rs:293-330`（ClientHandler 通知）+ registry watch（:565-679） | OK |
| onClose → failed+stop+status | index.ts:404-411 | registry.rs:594-612（同序，late-callback 身份校验） | OK |
| 状态机 pending/connected/failed/needs_auth/disabled + `unavailable()` 文案 | index.ts:65-76,427-481 | `registry.rs:275-286`（文案逐字）+ start_server（:414-484） | OK |
| reconcile（首启异步不阻塞；diff applied 增删替） | index.ts:537-575 | `registry.rs:717-786` | OK |
| add/remove/connect/disconnect（override 表 + disabled 语义） | index.ts:635-662 | `registry.rs:788-826`（overrides BTreeMap） | OK |
| 共享远端 URL 串行化（endpointLoads 锁） | index.ts:452-454 | `registry.rs:448-455`（endpoint_loads） | OK |
| 工具命名/命名空间 + codemode 开关 + 「invisible-direct」 | mcp.ts/tool | `registry.rs:1225-1246`（namespace/tool_name）+ `Tool::codemode_enabled`（:86） | OK |
| instructions 列表 + `<mcp_instructions>` 会话注入（codemode=false 服务器不加 execute 提示；diffByKey 增量） | mcp/instructions.ts:11-60 | `registry.rs:932-951` + `session/runner/context.rs:515-542`（tests_misc.rs:802 金样） | OK |
| elicitation（create/complete；表单字段映射 applyDefaults） | client.ts:96-112 | `elicitation.rs`（RegistryElicitation over FormService，JSON-Schema→表单字段映射 :283-474） | OK |
| roots/list 回答 cwd | client.ts:206 | `client.rs:293-297`（Handler 常答 roots_directory） | OK |
| **远程 OAuth**（401→NeedsAuth→登录：CIMD `CLIENT_METADATA_URL`、发现、PKCE loopback、token 存储/refresh 去重、loggedFetch） | `mcp/oauth.ts`（532 L）+ index.ts connectProvider | **MISSING**：仅 bearer 查询 seam（`registry.rs:181-253`，数据库 credential 行取 accessToken）；无 token 时 401 → `needs_auth` 死端（注释自认 Phase 9 scope） | **MISSING（D-30, S1, KNOWN—auditE §1.11 + 代码注释）** |
| 集成身份注册（remote+oauth 未禁用 → `Integration.ID(mcp_<sha1(name\0url)[0..16]>)`，凭据入全局库） | index.ts:176-215 | `registry.rs:260-271,506-521`（git2 SHA-1 blob hash 复刻 digest；集成行本体留给 integration 服务） | OK（行落地 PARTIAL：依赖 §3.8 的 Phase 5 注释） |
| **凭据切换重连**（Credential.Event.Switched → 停启对应服务器） | index.ts:577-598 | 无订阅（registry.rs 无 credential-switched 监听） | **MISSING（D-34, S2, NEW）** |
| 插件注册的 MCP 服务器进注册表 | mcp.transform → State | 快照收集但**不喂 config_source**（见 D-33） | PARTIAL |
| codemode 默认表（executor.sh→false；PostHog x-posthog-mcp-mode: tools） | plugin/mcp-codemode-defaults.ts | `codemode_defaults.rs`（URL 解析手写复刻，含默认端口剥离；registry.rs:1459 apply_codemode_defaults 于 boot 与 seed） | OK |

### 3.8 集成（integration）

| 能力 | v2 | Rust | 状态 |
|---|---|---|---|
| 注册表（Ref/Method{oauth,command,key,env}/implementations）+ transform 域 | integration.ts:135-155,296-340 | `integration/mod.rs:235-289`（update_integration/update_method/remove_method）+ 内置安装 | OK |
| list/get（含 methods + 当前连接投影 + 运行时 status） | :157-165 | `mod.rs:351-419` | OK |
| connection.active/resolve（key/oauth/env→可用凭证）/key（表单校验+存凭证）/activate/update/remove/status | :165-198 | `mod.rs:421-573` + `credential.rs`（active/replacement 挑选 SQL 逐字对齐 credential.ts:84-293） | OK |
| oauth.connect/status/complete/cancel（auto+code 双模式、CodeRequiredError、10min 尝试/1min 终态保留/30s 清扫、settle 持久化） | :199-220,237-239,558-608 | `mod.rs:574-747`（consts :35-39 同值；auto fork+settle） | OK |
| command.connect/status/cancel（外部命令登录） | :221-232 | `mod.rs:830-1010`（which 解析、进程、attempt 状态机） | OK |
| token 刷新（到期前 5min；JWT claims 解析） | :710 附近 | `mod.rs:38-39 REFRESH_MARGIN_MS` + `flows.rs:52`（jwt_claims） | OK |
| OAuth 流原语（PKCE S256、loopback 监听、device-code 轮询、bootstrap/success 页） | oauth/page.ts + provider 插件内 | `flows.rs`（pkce_s256:34、listen_pinned:80、listen_loopback:232、request_device_code:392、poll_device_token:433、页面 :204-232） | OK |
| well-known 发现（`/api/experimental/integration/wellknown`） | integration.ts + 协议 | `wellknown_config.rs`（207 L）+ 路由（integration.rs:58-61） | OK |
| Console（opencode zen）连接 | console 相关 | `console.rs`（316 L） | OK |
| HTTP 面（9 路由） | protocol/groups/integration.ts | `oc-server/src/handlers/integration.rs:54-94`（全 9 条） | OK |
| **内置 provider 集成清单** | 插件注册：`opencode`（zen）、`github-copilot`（device）、`gitlab`（pkce）、`azure`（cli+browser）、`openai`（api-key）+`chatgpt`（token-sharing/browser）、`xai`（device）、`digitalocean`（browser）、`poe`、`snowflake-cortex`（key/env/browser）、`modal`、`cloudflare-ai-gateway`（key）、`cloudflare-workers-ai`、`amazon-bedrock`（env AWS_BEARER_TOKEN_BEDROCK） | `providers.rs`+`providers_enterprise.rs` 安装 9 个：azure、digitalocean、github-copilot、gitlab、openai（含 chatgpt-token-sharing）、opencode、poe、snowflake-cortex、xai | **PARTIAL（D-40, S2, NEW：缺 modal / cloudflare-ai-gateway / cloudflare-workers-ai / amazon-bedrock 四项；auditE §1.8 的「整体缺失」结论已被 integration lane 落地推翻——本报告确认主体在）** |
| 插件注册第三方集成方法（ctx.integration.transform） | host.ts:274-373 | 无 ctx.integration | MISSING（S2；架构位：`IntegrationHost` 增设 host 侧注册 API 未开给 guest） |

### 3.9 权限（permission）

| 能力 | v2 | Rust | 状态 |
|---|---|---|---|
| `evaluate`（多 ruleset 扁平 findLast，action/resource 通配，缺省 ask） | permission.ts:87-97 | `permission.rs:60-69`（`oc_util::wildcard`） | OK |
| `merge`/`configured`（agent 缺省 deny-all；session 追加覆盖） | :99-102,158-163 | `permission.rs:71-82`（RuleResolver seam :250-259） | OK |
| deny 短路（任一 resource deny → 整体 deny，**不查 saved**） | :165-167,173-176 | `permission.rs:90-94,428-430` | OK |
| saved grants 追加最后（allow 规则）+ 项目域 | :148-156 | `saved.rs`（Memory）+ `sql.rs`（SQLite `permission` 表，v2 schema 注释逐字） | OK |
| `ask`（不泊车；ask 效果泊车+发布）/`assert`（deny=BlockedError（relevant 规则）；allow 直通；ask 泊车等回复） | :223-263 | `permission.rs:499-529` | OK |
| Declined（die 隧道，不得成为模型可见输出）/Corrected（带 feedback 的类型化失败） | :64-68,246-253 | `permission.rs:96-112`（文档注释同款语义约束） | OK |
| `reply`：先发 Replied 再结算；reject 批量同 session 兄弟（feedback 全批）；always+save 持久 grant 后**重评估批量放行**其余 pending | :265-325 | `permission.rs:530-620`（含 batch 逻辑） | OK |
| close（关闭时全部 reject + Declined） | :133-146 | `permission.rs` close（AtomicBool） | OK |
| `get/forSession/list` | :327-337 | `permission.rs`（pending BTreeMap 保持插入序注释） | OK |
| 事件（permission.asked/replied 顺序：replied 先于 deferred 唤醒） | bus publish await | `EventSink` trait 注释明确该约束（`permission.rs:261-274`） | OK |
| `permission.evaluate` hook override | :179-188 | seam 存在（HookInput/HookVerdict/HookOverride :222-245；evaluate_input :439-457）**生产 `None`** | MISSING 接线（D-24） |
| HTTP 面（request/saved/saved/:id/session 权限 6 路由） | protocol/groups/permission.ts | `handlers/permission.rs:19-25`（全） | OK |

### 3.10 表单（form）

| 能力 | v2 | Rust | 状态 |
|---|---|---|---|
| create/ask/get/list(pending,sessionID 过滤)/state/reply/cancel/close（关闭全 cancel） | form.ts:85-231 | `forms.rs:83-337`（含 Cache 语义→TTL 保留 10min 对应 RETENTION） | OK |
| 错误面 NotFound/AlreadySettled/AlreadyExists/InvalidAnswer/InvalidForm | :37-68 | `forms.rs:20-49` | OK |
| 字段类型 string/number/integer/boolean/multiselect/external + when 条件（eq/neq，数组 any）+ 级联失效 | schema/form + :259-270 | `oc-schema/src/form.rs:45-167` + `forms.rs` isActive/matches | OK |
| 校验：required/minLength/maxLength/pattern/format(email,uri,date,date-time)/options(!custom)/min/max/integer/minItems/maxItems；创建期 when 引用校验（前向引用/类型匹配/闭集选项） | :276-356 | `forms.rs:442-523`（validate_fields/validate_when）+ `validate_field` :523+ | OK |
| MCP elicitation→表单桥 | client.ts | elicitation.rs | OK（§3.7） |

---

## 4. 差异明细（EXPECTED vs ACTUAL）

> 格式：编号 ｜ 状态/严重度 ｜ KNOWN/NEW。行号以当前工作区为准。

- **D-01 ｜ MISSING ｜ S1 ｜ NEW** — 插件目录自动发现缺失。
  EXPECTED：config roots（global/project/ancestor 文档目录）下每 entry 的 `plugin|plugins/` 子目录内文件/目录/符号链接自动作为 add 操作（`ref core/src/plugin/source-directory.ts`（names `["plugin","plugins"]`）+ `core/src/config/plugin/source.ts:122-133`，且显式配置后置以使其 remove 可覆盖自动发现 :158）。
  ACTUAL：`oc-server/src/plugins.rs:294-314` 与 `oc-tui/src/plugin_boot.rs:150-172` 仅收集文档 `plugins[]` 数组；全仓无 `plugin`/`plugins` 目录扫描。
  影响：drop-in 式本地插件（无需改 config）完全不加载；v2 用户的 `~/.config/opencode/plugin/` 布局失效。

- **D-02 ｜ PARTIAL ｜ S2 ｜ NEW** — 运行时热重载未接线。
  EXPECTED：config 变更流 + 模块 digest 变化 + 24h 周期 + `plugin.updated` 事件触发再激活（`ref core/src/plugin/supervisor.ts:196-213`）；本地 import 图 digest 扫描（`packages/plugin/src/source.bun.ts`）。
  ACTUAL：digest 扫描与 generation cache-bust 已完整移植（host.js:1234-1262、`PluginHost::reload_now` lib.rs:515-519），但**无任何生产调用方触发**（grep `reload_now` 仅 oc-plugin-host 内部/测试）；无 config watcher、无 24h、无事件驱动。
  影响：改插件代码需重启进程（v2 保存即生效）。

- **D-03 ｜ STUB ｜ S2 ｜ KNOWN（auditE §1.4 🟡）** — `/api/plugin/check`、`/api/plugin/update` 空转。
  EXPECTED：npm registry 检查 + 24h 缓存 + keyed 互斥 + changes 流（`ref core/src/plugin/update.ts`），更新后 revision 变化触发再激活；`Plugin.Source.outdated/updating` 反映到 `/api/plugin`。
  ACTUAL：`oc-server/src/handlers/misc.rs:152-174` 读取并丢弃 body，分别返回空数组/no-content；运行时无 outdated 跟踪（`plugins.rs:182-184` 恒 `None`）。CLI 侧 `opencode plugin update` 有真实安装器（oc-cli/handlers/plugin.rs:256-322）。
  影响：UI 的「插件可更新」态不可用；更新须走 CLI。

- **D-04 ｜ PARTIAL ｜ S2 ｜ NEW** — guarded 插件 id 的 remove 保护缺失。
  EXPECTED：remove 操作跳过 `opencode`（zen）与 `opencode.policy`（`ref core/src/plugin/internal.ts:269-271` guarded 集合）。
  ACTUAL：`oc-plugin-host/src/resolve.rs:127-165` 的 remove 归约不查任何保护表；`oc-core/src/hooks.rs:1341-1344` `is_reserved_plugin_id`（"opencode." 前缀）**全仓无生产调用方**（仅定义与测试）。
  影响：repository config 可用 `-<id>` 移除本应受保护的内建语义位（Rust 内建已 native 化，实际可移除对象仅 guest 插件，故降为 S2）。

- **D-05 ｜ PARTIAL ｜ S2 ｜ NEW** — v1 插件形态仅「容忍忽略」。
  EXPECTED：v1 `server()` 导出经 v1 兼容层获得等效能力（`ref core/src/v1`）。
  ACTUAL：host.js:853-857 classify 识别 v1 后仅记日志 "tolerated (ignored)"，不加载。
  影响：仅 v1 老插件受影响（生态已 v2 化，S2）。

- **D-07 ｜ PARTIAL ｜ S1 ｜ KNOWN（代码内自述 documented divergence，`handlers/misc.rs:141-146`）** — 插件激活从 per-location 变为 process-global。
  EXPECTED：每 location 一套插件注册表与激活代（`ref core/src/plugin.ts:17-257`，LocationGraph 节点；跨 location ctx 调用经 `locationRef` 路由 host.ts:83-112）。
  ACTUAL：单一 bun 子进程 + 单一注册快照服务所有 location；`ctx.location` 固定为 boot 目录（host.js:427）。
  影响：多 location（多项目 worktree）下插件看到单一 location 视图；v2 的 location 隔离与 per-location 重载不适用。

- **D-10 ｜ PARTIAL ｜ S1 ｜ KNOWN（端口内自述 TODO）** — `ctx.session` 面大幅缩水。
  EXPECTED：15 方法 + hook（`ref core/src/plugin/host.ts:527-569`；`packages/plugin/src/promise/session.ts:154-170`：create,get,remove,switchAgent,switchModel,prompt,generate,command,compact,synthetic,interrupt,update,move,wait,context）。
  ACTUAL：guest 暴露 11（host.js:573-586），服务器端口仅实现 prompt/get/list/messages/interrupt（`oc-server/src/plugins.rs:525-584`，其余返回 "not wired in the Rust server port yet"）；create/switchAgent/command/compact/move 未暴露；TUI embedder 走 HTTP 客户端端口（plugin_boot.rs HttpSessionPort）覆盖面另计。`list`/`messages` 为 v2 插件 API 没有的 EXTRA。
  影响：依赖会话编排的插件（自动创建子会话、compact、move 等）不可用。

- **D-11 ｜ STUB ｜ S1 ｜ NEW** — `ctx.permission` 仅 reply 且为空响应。
  EXPECTED：list/get/reply 三方法 + `permission.hook("evaluate")`（`ref host.ts:404-427`；`promise/permission.ts:20-24`），reply 作用于真实 pending 请求。
  ACTUAL：guest 仅 `permission.reply`（host.js:599-603），supervisor 收到后**直接回 null 不路由**（`oc-plugin-host/src/supervisor.rs:307-311` 注释自认「wave 4 seam」）；list/get/hook 无。
  影响：插件无法实现权限自动应答/审批 UI 类功能。

- **D-13 ｜ PARTIAL ｜ S1 ｜ NEW** — 服务器侧插件事件订阅无事件源。
  EXPECTED：`ctx.event.subscribe()` 看到全部 ServerEvent + `rpc.*`（`ref host.ts:255-264`，EventManifest 过滤）。
  ACTUAL：事件分发泵只消费 `BroadcastTap`（supervisor.rs:1496-1530），而 tap 只有 TUI embedder 接（`oc-tui/src/plugin_boot.rs:253` store tap；dedup 环 tap.rs）；**oc-server 的 `PluginBoot` 从不向 tap 发布**（grep `tap()` 在 oc-server 零命中）→ serve 模式下插件 `event.subscribe` 收不到任何事件；TUI 模式可收 TUI 总线事件（无 `rpc.*`，类型过滤在 guest 内按订阅进行，无 ServerEvent 白名单——会多送 TUI 内部事件）。
  影响：serve/headless 下事件驱动插件完全失效；TUI 下基本可用。

- **D-14 ｜ STUB ｜ S1 ｜ NEW（auditD 仅记录路由存在，未判 stub）** — 插件 RPC 全链缺失。
  EXPECTED：`ctx.rpc` client/register（`ref host.ts:119`；`core/src/rpc.ts:69-175`：schema 校验、声明式错误、`rpc.<id>.<event>` 事件、多注册末位胜出）；HTTP `POST /api/rpc/:rpcID/:method` 可调用。
  ACTUAL：guest ctx 无 `rpc` 域；`oc-server/src/handlers/misc.rs:180-188` 恒 `rpc.internal`；`oc-core/src/hooks.rs:1272-1313` 仅 trait 面（`TODO(plugin-lane)`）；`rpc.*` 事件无发布者。
  影响：以 RPC 为扩展面的插件（如桌面伴侣类）不可用；客户端 `/api/rpc` 调用全部 500。

- **D-15 ｜ MISSING 接线 ｜ S2 ｜ NEW** — `shell.create.before` 无生产触发。
  EXPECTED：Shell.create 前 trigger（`ref core/src/shell.ts:274`），插件可改写 command/cwd/env/timeout。
  ACTUAL：`oc-core/src/shell.rs:432-454` 有 `before: Option<BeforeHook>` 同步闭包参数（生产传 None）；`HookRegistry::dispatch_shell_create_before`（hooks.rs:890-891）无调用方；guest 无 `ctx.shell`。
  影响：shell 审计/改写类插件失效。

- **D-16 ｜ PARTIAL ｜ S2 ｜ NEW** — tool 编辑器缺 `namespace()`。
  EXPECTED：`ToolEditor.namespace(ns)` 设工具命名空间（`ref packages/plugin/src/promise/tool.ts:31`），影响 Code Mode `tools["ns"].*` 树。
  ACTUAL：host.js:242-258 editors.tool 仅 list/get/add/update/remove；`PluginToolDef.namespace` 字段存在（plugin_apply.rs:66）但 guest 编辑器无法设置。
  影响：插件工具全部落在默认命名空间。

- **D-18 ｜ WRONG ｜ S1 ｜ NEW** — 插件 KV 非持久。
  EXPECTED：`plugin:<hex(id)>:` 前缀写入 KV.Service（DB 持久，跨重启保留；`ref core/src/plugin/host.ts:602-629`）。
  ACTUAL：`oc-plugin-host/src/supervisor.rs:1560-1577` StorageFile 落 `<sandbox>/plugin-storage/<id>.json`，sandbox 为 `/tmp/opencode/oc-plugin-host-<pid>-<n>-<nanos>`（`lib.rs:813-821`，每次进程启动重建、/tmp 生命周期）；另有 key/value 大小上限（EXTRA 约束）。oc-core `PluginStorage` trait（hooks.rs:1261-1266，注释「DB KV 表 + plugin-ID 域」）未与该实现合流。
  影响：插件状态（onboarding 标记、缓存、侧栏折叠态等）重启即失——对旗舰插件的行为保真构成可见回归。

- **D-21 ｜ MISSING 接线 ｜ S1 ｜ KNOWN（根 FINDINGS.md §5「engine-side adapter … does not [exist]」；ROADMAP 行尾却称 "wired in the engine lane"，与代码不符，见 §5）** — guest 插件 hooks 不进引擎分发链。
  EXPECTED：插件注册的 session/tool/permission/shell hook 在引擎触发点生效（v2 同进程天然成立）。
  ACTUAL：链路三件套齐备——guest `session.hook/tool.hook`→`hook.register`（supervisor.rs:279-296）、快照 `hooks`（registrations.rs:120-126）、`Bridge::dispatch_hook`/`hook_subscribers`（context/mod.rs:112-150，e2e 测试使用）——但 `PluginSupervisor.host_slot: ()`（`oc-core/src/hooks.rs:1423`，boot 时 `let () = self.host_slot;` :1506-1508 明示「nothing to boot in Wave 0」），引擎所有 dispatch 只走进程内 `HookRegistry`，无任何 guest 桥接注册。
  影响：**JS 插件的全部 hook 能力在生产不生效**（含 `tool.execute.before` 拒绝、`session.context` 改写、`permission.evaluate` 覆盖）。这是插件平台当前最大的单一断点。

- **D-22 ｜ WRONG ｜ S2 ｜ NEW** — `session.generate` hook 被并入 `session.context` 触发。
  EXPECTED：一次性 generate 有独立 `generate` trigger（`ref core/src/session/model-request.ts:410`）。
  ACTUAL：`oc-core/src/session/runner/model_request.rs:537-549` 仅 Compaction 分支独立，`_ =>`（Primary 与 Generate）统一 dispatch `session_context`；`generate.rs` 无 dispatch。
  影响：注册在 `session.generate` 的 handler 不触发；注册在 `session.context` 的 handler 会额外看到 generate 请求。

- **D-23 ｜ MISSING 接线 ｜ S2 ｜ KNOWN（FINDINGS.md D-03 表 (a) 类「runner wrap unwired yet」）** — `session.http.request/response` 与 ws 三点无 dispatch 站点。
  EXPECTED：请求/响应改写钩子在传输 wrap 内触发（`ref model-request.ts:326-338`）。
  ACTUAL：事件类型齐（hooks.rs:476-525），无生产调用（oc-ai 传输层未接）。

- **D-24 ｜ MISSING 接线 ｜ S2 ｜ KNOWN（FINDINGS.md「production dispatch site unwired」）** — `permission.evaluate` hook override 生产为 None。
  EXPECTED：评估管线第 4 步 trigger（`ref core/src/permission.ts:173-189`），非 deny 短路路径必经。
  ACTUAL：`oc-core/src/engine/mod.rs:705-720` `Permission::new(..., None)`；`HookOverride` seam（permission.rs:244-245）与 `dispatch_permission_evaluate` 均无生产调用方。

- **D-30 ｜ MISSING ｜ S1 ｜ KNOWN（auditE §1.11 + `registry.rs:181-186` 注释自认 Phase 9）** — 远程 MCP OAuth 登录流缺失。
  EXPECTED：401 → `NeedsAuthError` → 集成身份 + CIMD/发现/PKCE loopback/凭据存储/refresh 去重（`ref core/src/mcp/oauth.ts:28-330`、index.ts:176-215 connectProvider、client.ts:263-275 authProvider）。
  ACTUAL：仅 DB credential 行 bearer 查询（`registry.rs:181-253`）；无 token 时连到 401 服务器即 `needs_auth` 死端（`/mcps` 提示 Sign in 但无登录通道）。
  影响：所有 OAuth 类远程 MCP（Cloudflare 等）不可用；API-key/匿名/本地服务器不受影响。

- **D-31 ｜ MISSING/PARTIAL ｜ S2 ｜ NEW** — (a) `protocol: legacy|auto|pin` 协商开关与 `UnsupportedProtocolVersionError` 提示缺失（rmcp 自带协商，无 pin 面）；(b) 旧式 SSE-only 端点（独立 `/sse` GET 传输）无专用传输——rmcp StreamableHTTP 仅处理 streamable + 响应侧 SSE 流。
  EXPECTED：`ref mcp/client.ts:184-199,392-397`（negotiation/错误文案列出服务器支持版本）。

- **D-33 ｜ PARTIAL ｜ S1 ｜ NEW** — 插件注册的 MCP 服务器不进真实注册表。
  EXPECTED：`ctx.mcp.transform` 的结果进入 MCP State 并 reconcile 启动连接（`ref host.ts:374-403`）。
  ACTUAL：快照收集 `mcps`（registrations.rs:309-314 merge_mcps；host.js 编辑器+seed 完整），但 `oc-server/src/plugins.rs:241-269` 的 HostState.mcps 只含**config 文档 + codemode 默认表**，`HostState`/`McpRegistry.config_source` 均不合并快照 `mcps`；引擎侧 `LocationPluginSource` 也无 mcp 项（plugin_apply.rs:77-107）。
  影响：插件无法提供/改写 MCP 服务器配置（guest 内 `ctx.mcp.list` 可见 seed，纯展示）。

- **D-34 ｜ MISSING ｜ S2 ｜ NEW** — 凭据切换不触发 MCP 重连。
  EXPECTED：订阅 `Credential.Event.Switched`，属于本 registry 注册集成的凭据变化 → 停启该服务器（`ref core/src/mcp/index.ts:577-598`）。
  ACTUAL：`oc-core/src/mcp/registry.rs` 无 credential 事件订阅（grep 无 switched）。
  影响：MCP OAuth/凭据更换后需手动 reconnect（叠加 D-30 后当前主要影响预置 token 场景）。

- **D-35 ｜ PARTIAL ｜ S1 ｜ NEW** — 插件注册的 skills 不进引擎技能表。
  EXPECTED：`ctx.skill.transform` 进入 Skill State（`ref host.ts:444-457`）。
  ACTUAL：快照收集 `skills`（supervisor.rs:614-621；host.js 编辑器齐），引擎 `ConfigSkillTable` 只做 config 发现（`engine/mod.rs:753-754`；`engine/skills.rs:79-81` 注释自认「all skills enter … no skill-editor plugin port」），`apply_location_plugins`（plugin_apply.rs:122-151）无 skills 步骤。
  影响：插件捆绑技能（旗舰的 SKILL.md 文档类）不可被 skill 工具/发现面看到。

- **D-40 ｜ PARTIAL ｜ S2 ｜ NEW** — 内置集成 provider 集合缺 4 项。
  EXPECTED：v2 provider 插件注册 14 个集成（§3.8 表）。
  ACTUAL：`oc-core/src/integration/providers.rs` + `providers_enterprise.rs` 安装 9 个；缺 `modal`、`cloudflare-ai-gateway`（key）、`cloudflare-workers-ai`、`amazon-bedrock`（env AWS_BEARER_TOKEN_BEDROCK）。
  影响：这 4 家 provider 无法经 `/connect` 登录（auditE §1.8 的整体 ❌ 已被推翻，此处为残余缺口）。

- **EXTRA 汇总（Rust 增量，正向/中性）**：子进程隔离 + 崩溃归因打击禁用（supervisor.rs:473-530）；死锁协议与超时界（lib.rs:29-56，2s 注册/5s 分发默认）；托管 bun bootstrap（bunman pinned）；`OPENCODE_DISABLE_PLUGINS` 总开关（plugins.rs:194-196）；Code Mode 沙箱化到 guest（v2 为进程内受限解释器——**注意这是放宽**：guest 内 imports/timers 可达，host.js:1014-1020 注释自认 documented divergence）；tap 去重环（G11-F1）；UI slot/dialog/group/home 桥（对应 v2 TUI feature-plugin 面）；`hostFlavor` 标记；storage key/value 上限。

---

## 5. 与 ROADMAP / FINDINGS / docs 的对照（KNOWN 判定与冲突）

| 本报告项 | 文档依据 | 判定 |
|---|---|---|
| D-03 hook copy-in/copy-out 及 18 点位语义 | 根 `FINDINGS.md`（D-03 spike 全文）+ `ROADMAP.md` D-03 行 | KNOWN（已定案；本报告核验 hooks.rs 实现与文档一致） |
| D-21 guest→HookRegistry adapter | 根 `FINDINGS.md:58-63`（"does not [exist] … wiring them into the …"）；`ROADMAP.md` D-03 行尾称 "wired in the engine lane" | **KNOWN + 文档冲突**：代码证据（`host_slot: ()`、dispatch_hook 零生产调用）支持 FINDINGS 的「未接线」；ROADMAP 的 "wired" 表述失准（或仅指 spec/测试），建议更正 |
| D-30 MCP OAuth 流 | `docs/auditE-core.md` §1.11（❌ flow）+ `registry.rs:181-186` 注释（Phase 9 scope）；`docs/spec-round1.md:358` C10 | KNOWN |
| D-03'（plugin check/update stub） | `docs/auditE-core.md` §1.4（🟡 server routes no-ops） | KNOWN |
| 内建 provider/warming/optimize/vcs/websearch 插件 native 化缺口 | `docs/auditE-core.md` §1.4/§1.7/§1.8 | KNOWN（本报告 §3.3 相应行引用；warming/optimize/系统提示词族属模型 lane，未重复展开） |
| 集成 host | `docs/auditE-core.md` §1.8 判 ❌ absent | **过时**：`crates/oc-core/src/integration/`（5948 L）已落地并接 HTTP 9 路由——本报告确认主体 OK，残余 D-40 |
| `command.list` 含插件命令 | `docs/auditE-core.md` §1.4（🟡 route 返回 []） | **已修复**：`handlers/agent.rs:55-79` 经 CommandTable 合并 `register_plugin`（CD-12/CD-34） |
| /api/rpc、plugin.check/update、permission/mcp/integration 路由存在性 | `docs/auditD-api.md`（141/141） | KNOWN（路由在；本报告补充其中 rpc/check/update 为 stub 的**行为级**结论——对 auditD 是 NEW 增量） |
| TUI 插件面（slots/sidebar/dialog/theme/keymap） | `docs/parity-scan-2026-10-18/06-tui.md`、SCAN-AB/RESCAN 系列、ROADMAP 2026-10-17 条目 | KNOWN（本报告仅列 ctx 面归属，细节不重复） |
| Code Mode guest 沙箱放宽 | host.js:1014-1020 注释 | KNOWN（documented divergence） |
| 其余标注 NEW 的项（D-01/02/04/05/10/11/13/14/15/16/18/22/31/33/34/35/40） | 未见于 ROADMAP/FINDINGS/docs 既有记录 | **NEW** |

---

## 6. 覆盖清单（本次通读/取证的范围）

**参考侧（/opt/opencode-ref）**：`packages/core/src/plugin.ts`（300 L 全读）；`plugin/` 下 host.ts(676)、hooks.ts(108)、internal.ts(287)、supervisor.ts(258)、tool-input-repair/update/warming/verbosity/optimize/plan/agent/command/skill/provider/models-dev/mcp-codemode-defaults/identity/instance/sdk/service/source-directory/module/promise.ts（标题+关键行）；`plugin/provider/`（34 文件清单+集成注册 grep）、`plugin/vcs|websearch|command|skill|system-prompt`（清单）；`config/plugin/source.ts`（197 全读）；`packages/plugin/src/`（host.ts、source.ts、source.bun.ts、source.package.ts、storage.ts、rpc.ts、options.ts、worktree.ts、promise/{index,adapter,session,tool,permission,integration,mcp,...}.ts、effect/ 清单、tui/ 清单）；`core/src/rpc.ts`（289 全读）；`mcp/{client,index,instructions,oauth,stdio}.ts`（oauth 头 80 行+导出清单；client/index 关键段全读）；`integration.ts`（834 中 1-340 全读+grep）、`integration/connection.ts`（存在性）；`permission.ts`（347 全读）、`permission/{saved,sql}.ts`（存在性）；`form.ts`（370 全读）；`oauth/page.ts`；`protocol/src/groups/{integration,permission,mcp,plugin}.ts`（路由清单）。

**Rust 侧（/opt/opencode-rust）**：`crates/oc-plugin-host/src/`（lib.rs 862 全读、supervisor.rs 关键段、host/host.js 1862 全读、protocol.rs 全读、registrations.rs 结构、resolve.rs 428 中 1-160、context/{mod.rs 关键段,install.rs 存在性,codemode_defaults.rs 1-80}、tap.rs 头 60、ui.rs/bun.rs/host_js.rs 存在性；tests/ 清单）；`crates/oc-core/src/hooks.rs`（2071 中 1-720+1041-1519 全读）；`plugin_apply.rs`（206 全读）；`mcp/{mod,client,registry,elicitation}.rs`（client connect/start 全读、registry 260-1040 全读、elicitation 结构）；`integration/{mod,types,flows,credential,providers,providers_enterprise,wellknown_config}.rs`（mod 560-680、flows/credential 头部+结构、providers id 清单）；`permission.rs`（1-530 全读）；`permission/{saved,sql}.rs`（结构）；`forms.rs`（结构+校验段）；`session/runner/{model_request,generate,compaction,retry}.rs`（dispatch grep+关键段）；`tool/registry.rs`（hook 段）；`shell.rs`（Invocation/create 段）；`engine/mod.rs`（权限/插件缝段）；`location.rs`/`location_services.rs`（hooks 接线段）；`oc-server/src/plugins.rs`（628 中 1-628 全读）、`handlers/{misc,agent,integration,permission,mcp}.rs`（路由/桩段）；`oc-tui/src/plugin_boot.rs`（106-260）；`oc-cli/src/handlers/plugin.rs`（结构）；`oc-schema/src/form.rs`、`oc-schema/src/mcp`（字段面）。交叉文档：`ROADMAP.md`、根 `FINDINGS.md`、`docs/auditD-api.md`、`docs/auditE-core.md`、`docs/parity-scan-2026-10-18/10-surfaces.md`。

**未深入（边界声明）**：TUI 插件 UI 呈现保真（06-tui.md 职域）；内置 provider 插件的模型 lane 语义（warming/optimize/system-prompt 族）；websearch 引擎接线；MCP stdio.ts 的 ref 细节（Rust 用 rmcp TokioChildProcess，行为面已对齐）；`PersistentPty.read` 实验面。

---

## 7. 结论与修复排序建议

**总体**：Rust 侧把「最难的三块硬骨头」——hook 注册表语义（18/18 点位 + 顺序/短路/provider 重读/失败传播）、MCP 客户端与注册表生命周期、集成 OAuth/命令/密钥三流 + 权限 + 表单——移植到了高保真水平（大量逐字文案/SQL/常量对齐，测试密度高）。当前平台的真实短板集中在**插件↔引擎的接线层**：插件能注册到的东西（hooks/skills/mcp/事件/RPC/permission），相当一部分在生产路径没有消费者。

建议排序（按用户可见收益）：

1. **D-21 guest hooks 引擎接线**（S1，最大单一断点；FINDINGS §5 已有完整 spec：按 (plugin,hook) 注册 HookHandler→`Bridge::dispatch_hook`）——顺带消化 D-24/D-15 的 dispatch 站点；
2. **D-18 插件 KV 落 DB**（S1，数据丢失级；oc-core 已有 `PluginStorage` trait 蓝 图，supervisor 换后端即可）；
3. **D-13 serve 模式事件 tap 接线**（S1；把 bus→tap 泵接进 `PluginBoot`，沿用 G11-F1 去重）；
4. **D-01 plugin/plugins 目录发现 + D-02 reload_now 挂 config watcher**（S1→S2 组合，加载面补全）；
5. **D-33/D-35 snapshot.mcps/skills 进引擎**（S1；`LocationPluginSource` 加两方法即可复用现有 apply 管道）；
6. **D-30 MCP OAuth 登录流**（S1，KNOWN Phase 9；flows.rs 已有 PKCE/loopback/device 原件，缺 CIMD/发现 + registry 接 authProvider 等价物）；
7. **D-14 RPC、D-10/D-11 session/permission 端口补全、D-22 generate 独立触发、D-40 集成补 4 家**（S1-S2 清账）。
