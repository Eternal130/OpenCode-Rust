# L6 MCP / Plugin / Codemode — 审查报告

> 审查方式：ref ~13.4K 行含 plugin 包、Rust ~20K 行核心 + oc-server 装配层抽查，逐文件阅读。结论均出自实读。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明
- **Ref 全读**：`mcp/{index,client,stdio,oauth,instructions}.ts`（2103 行）、`plugin/{host,supervisor,module,instance,sdk,service,update,hooks,internal,source-directory,mcp-codemode-defaults}.ts`、`plugin.ts`、`codemode/{tool,catalog,web,instructions}.ts`、`packages/plugin/src/{host,source,source.bun,source.node,...}` 及 effect/promise/tui 目录清单。
- **Rust 全读**：`oc-core/src/mcp/{mod,client,registry,oauth,oauth_page,elicitation}.rs`（约 6.5K 行）、`oc-plugin-host` 全部 src（lib/supervisor/registrations/resolve/tap/protocol/bun/host_js/ui/context 四件）、`oc-core/src/plugin_apply.rs`、`tool/codemode.rs`（关键 1200 行 + 结构 grep）、`oc-server/src/plugins.rs` 装配层定向核对。

### 2. 逐项对比表

#### MCP
| 功能点 | Ref (file:line) | Rust (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| local/remote 配置解析 | mcp/index.ts:78-99 | registry.rs:128-155,193-207 | ✅ | ServerEntry 结构一致 |
| 启动（含 tools 内联失败即 failed） | index.ts:427-481 | registry.rs:687-765 | ✅ | pending 公告+connect+tools 一体 |
| 五状态生命周期+startup latch | index.ts:479,65-76 | registry.rs:127-179,451-462 | ✅ | Notify latch 等价 |
| per-server 锁 + per-URL 端点串行 | index.ts:96,169,453 | registry.rs:468-471,729-736 | ✅ | |
| stdio 传输（cwd/env/BUN_BE_BUN/16MiB 帧上限/stderr tail） | stdio.ts:10-18,45-115; client.ts:247-259 | client.rs:1292-1444 + environment::stdio | ✅ | 帧上限移到 FrameCapReader seam（G-72） |
| 瞬态重试 250/1000ms + deadline | client.ts:171-182,55-58 | client.rs:539-554,1513-1567 | ✅ | 模块注释写"250/500"是笔误，代码正确 |
| codemode=false URL 参数 + 原始 URL 重试 | client.ts:267-292 | client.rs:1459-1465,1546-1555 | 🟡 | `error_400_404`(1570) 按错误文本含"400/404/Bad Request"匹配，URL 含这些数字会误判重试（低风险） |
| 会话过期识别（404/特定400 + session gate） | client.ts:228-244 | client.rs:499-515（G-223 typed） | ✅ | 用 SessionObservingClient 观察 session id，更严格 |
| 过期恢复 + 单次重放（recovering） | index.ts:340-365 | registry.rs:1216-1286 | ✅ | |
| **后台 list-changed 观察到过期的自动重连** | index.ts:415 onSessionExpired→fork(recover) | registry.rs watch（905-957）仅 close/list-changed | 🔴 | Rust 无独立 session-expired 回调：后台刷新失败不触发重连，仅靠前台调用重放路径恢复 |
| needs_auth 判定 | client.ts:383 | client.rs:1502-1508 preflight + connect_failure:1619-1626 | ⚠️ | 🔵新增 initialize 预检探针（rmcp 吞 401 的补偿），服务器会收到两次 initialize；行为面等价但多一请求 |
| 关闭：terminate DELETE 1s + close | client.ts:296-315 | client.rs:1146-1151 | ✅ | TERMINATE_TIMEOUT=1s 对齐 |
| 工具发现/命名空间前缀/`{server}_{tool}` | tool/mcp.ts:16-17 | registry.rs:1593-1604,1539-1554 | ✅ | |
| 工具桥：权限先行、structured→JSON 文本→raw、media→file、错误 join | tool/mcp.ts:43-117 | registry.rs:1619-1707 + 1714-1770 | ✅ | output 经 TOOL-D1 seam + metadata 双轨 |
| 100ms debounce 工具注册桥 | tool/mcp.ts:139 | registry.rs:44,1786-1917 | ✅ | 滑动窗 debounce + 初次 flush |
| 资源/提示/读资源 | index.ts:367-400,706-751 | registry.rs:1357-1456,1480-1535 | ✅ | 模板列表失败折叠为空、localecmp 排序 |
| elicitation form/url/legacy complete | index.ts:231-302,786-846 | elicitation.rs 全文件 | ✅ | number/integer 分列(G-280②)、machine-title 正则、pending guard Drop 全路径删除 |
| 现代 MRTR input_required 轮 | client.ts 内 SDK | client.rs:1040-1126 + elicitation result_wire | ✅ | 单一 execution deadline 覆盖全部轮次 |
| OAuth：发现/DCR/CIMD/PKCE/loopback/轮换/共享刷新 | oauth.ts 全文 | oauth.rs 全文 | ✅ | 刷新去重键用资源 URL 替代 token endpoint URL（已记录的等价偏差）；iss 校验 RFC9207(G-110②) |
| 凭据失效（invalidate→remove row） | oauth.ts:290-296 | registry.rs:811-825 + oauth.rs is_token_rejected | ✅ | 仅 grant 被拒才删行 |
| 集成注册 `mcp_<sha1(name\0url)[0..16]>` + 凭据切换重连 | index.ts:176-215,579-598 | registry.rs:436-447,852-865,588-633 | ✅ | git2 SHA-1 与 createHash 等值 |
| codemode 默认表（executor.sh/PostHog） | plugin/mcp-codemode-defaults.ts | registry.rs:1929-2027 + context/codemode_defaults.rs | ✅ | `?mode=` 与 header 大小写豁免均有 |
| mcp instructions | mcp/instructions.ts | oc-core session/runner/context.rs:517-599 | ✅ | core/mcp-guidance 权限过滤渲染/diff |

#### Plugin
| 功能点 | Ref | Rust | 状态 | 差异说明 |
|---|---|---|---|---|
| 架构：进程内 Effect 插件 vs bun 子进程 host | plugin.ts:47-87 | oc-plugin-host 全 crate | ⚠️ | 有记录的架构分歧（ADR）；行为面用端口/桥逼近 |
| **Effect 风格插件 API（@opencode/plugin/effect）** | packages/plugin/effect/*（21 模块） | host.js:1875-1882 显式拒绝 | 🔴 | G-148② 生态破坏性变更：import 即报错，仅支持 promise `{id,setup}` |
| 插件发现（plugin/ plugins/ 目录、ts/js/目录/symlink、排序） | source-directory.ts:9-33 | resolve.rs:123-173 | ✅ | |
| 配置 ops 解析（spec/{package,options}/-remove/-*） | supervisor.ts:18-114 | resolve.rs:43-69,179-227 | ✅ | |
| npm 包加载（解析安装→entrypoint 探测） | module.ts:80-133 | resolve.rs:249-312 + install.rs | ⚠️ | Rust 启动解析离线（未装=失败+提示 `plugin add`），ref 可 npm.add——安装移入 CLI 通道（行为分歧，有记录） |
| 入口解析（server/tui/rpc exports） | plugin/host.ts:17-44 | install.rs resolve_entry + supervisor probe_tui_entry | ✅ | |
| 热重载（digest 扫描+CJS 缓存逐出+generation cache-bust） | source.bun.ts:11-68 | host.js:2825-2853 + supervisor reload_changed | ✅ | `?t=` 仅 generation>1 |
| **失败重载保留旧代** | supervisor.ts:77-82 | supervisor.rs:1938-1966：先 clear_plugin+unload 再 load | ⚠️ | Rust 失败热重载会把旧注册一并清掉，ref 是新代成功才替换——有注册丢失风险 |
| 激活模型（前缀 diff、失败修订不重试、fallback 恢复） | plugin.ts:100-180 | supervisor 无对应 | 🔴 | 无 (id,revision) 代际机制 |
| transform 失败→禁用插件+刷新 | plugin.ts:50-63,182-236 | state_transform.rs 仅 agent 域 | 🟡 | 仅 agent transform 有分组失败隔离 |
| 崩溃归因/3 strike 禁用/8 次重启放弃/退避 | （无，Effect fiber 隔离） | supervisor.rs:1211-1269,2431-2537 | 🔵 | 移植版新增的监督策略 |
| 死锁协议（reader 不阻塞、写通道不持锁、全调用超时） | （进程内无需） | supervisor.rs §1 + host.js 镜像 | 🔵 | 2s 注册/5s 查询/120s 变更类（RC-03） |
| ctx 全域 API（agent/mcp/skill/tool/command/session/permission/shell/aisdk/vcs/provider/model/reference/websearch/worktree/integration/rpc/storage/experimental/generate/event/theme/ui/keymap） | plugin/host.ts:115-570 | host.js:840-1862 makeCtx | ✅ | 逐一对照均有；`ctx.storage` DatabasePluginStorage 端口 |
| hook 域清单与注册序触发 | hooks.ts:63-106 | host.js:765-825 + registrations.rs:799 + plugin_apply.rs:997-1130 | ✅ | session 10 项+tool 2+shell/permission/aisdk/websearch/worktree/integration；失败中断链一致 |
| hook tap 点：tool.execute.before/after、session.prompt/context/...、permission.evaluate、aisdk.sdk/language、shell.create.before、http/ws | hooks.ts:13-30; host.ts:157-188,405,460-467,528 | plugin_apply.rs:997-1121 全部接线 + is_mutating_hook 分级 | ✅ | copy-in/copy-out 序列化变异（ADR-0005 记录偏差） |
| 事件订阅（ServerEvent+rpc 过滤、async-iterable） | host.ts:255-265 | host.js:1530-1615 + tap.rs | ⚠️ | Rust tap 有界队列满即丢弃（记日志）；ref Stream 无界不丢；另有 512 深订阅队列溢出丢最旧 |
| mcp.transform 落引擎 | host.ts:393-403 | plugin_apply.rs:1200-1211 | ✅ | |
| per-location 激活 | supervisor.ts（location node） | plugin_apply.rs:199-218 PLUG-D07 | 🟡 | 单 guest 近似：ctx.location 跟随最新激活位置 |
| SdkPlugins/InstancePlugins 双通道 | sdk.ts / instance.ts | 无独立通道（快照+PLUG-D07 过滤） | 🟡 | 嵌入器注册路径合并进 boot 快照 |
| 插件更新检查/更新流（24h/outdated/updating） | update.ts | context/update.rs | 🟡 | `PluginUpdate.changes` 热流未见对等物 |
| 重载触发面（+100ms debounce） | supervisor.ts:196-237 | oc-server/plugins.rs:621-674 | ✅ | 装配层补齐 |
| 内建 pre/post 插件族 | plugin/internal.ts:213-267 | 不在本域（native 端口） | 🟡 | 其余内建属各功能域 |
| v1 `server()` 导出 | module.ts:60-73 会 LoadError | host.js:269-273 容忍并忽略 | 🟡 | 移植版更宽容 |

#### Codemode
| 功能点 | Ref | Rust | 状态 | 差异说明 |
|---|---|---|---|---|
| `execute` 工具合成+snapshot 切分 | codemode/tool.ts:62-140 | codemode.rs:71-122 | ✅ | 描述逐字节 |
| 沙箱执行 | 进程内受限解释器 | host.js:2640-2757 AsyncFunction+guest | ⚠️ | 真实 JS 引擎替代解释器：import()/require 靠静态扫描、timers 置换为抛错、`new Function/eval` 可绕过（已声明的静态扫描极限） |
| fetch 扩展（30s 超时+净化响应） | codemode/web.ts:5-46 | host.js:2433-2461 | ✅ | 嵌套执行栈正确恢复 |
| 结构上限（string/array/pending/depth） | interpreter/limits.ts | host.js:104-125,2155-2162 + codemode.rs | ✅ | 精确 RangeError 文案 |
| ExecutionLimits（无默认、校验文案） | codemode.ts:22-41,126-137 | codemode.rs:1133-1178 + host.js:2131-2144 | ✅ | timeoutMs 双层（软超时+supervisor 杀进程豁免 strike） |
| search 内建（tokenize/termForms/打分/limit10） | tool-runtime.ts | host.js:2264-2344 + codemode.rs searchText | ✅ | |
| 目录 summarize（2000 预算、pinned 预付、UTF-16 计量） | catalog.ts:48-189 | codemode.rs:561-750 | ✅ | G-106③④ UTF-16 修正 |
| 指令渲染/增量更新 | codemode/instructions.ts:8-141 | codemode.rs:752-1108 | ✅ | 字节级对齐含空目录文案 |
| 嵌套结果优先级；文件收集 | codemode/tool.ts:93-127,329-342 | codemode.rs nested_value + host.js | ✅ | |
| progress 行（toolCalls 起止/fetch 行） | codemode/tool.ts:86-89,143-176 | codemode.rs:1694-1899（G-74 单次聚合 emit） | 🟡 | ref 每次 start/settle 都 emit；Rust 一次聚合发射（频次差异） |
| console 捕获进 logs | stdlib/console.ts | host.js:2353-2426 | ✅ | G-180① |

### 3. 关键差异与风险 Top 5
1. **热重载先卸后装**（supervisor.rs:1942-1951）：`reload_changed` 在新 load 成功前就 `clear_plugin`+`unload` 旧代；失败时注册已丢。Ref 保留运行中代。建议改为 load 成功后再切换。
2. **后台 session 过期不重连**（registry.rs:905-957）：无 `onSessionExpired` 通知路径，list-changed 推送里的过期只能等下一次前台调用重放；Ref 主动 recover。长时间空闲的远程 server 状态会陈旧。
3. **Effect 插件 API 整族缺失（G-148② 有意为之）**：所有 `@opencode/plugin/effect` 生态插件在 Rust 宿主不可用且直接报错——最大的生态兼容断层。
4. **代际激活模型缺失**：无 (id,revision) 前缀 diff、无失败修订不重试、无 fallback 恢复。Rust 失败 entry 会在每次 reload pass 重试，且无降级回退语义；配合风险 1 放大。
5. **事件 tap 有界丢弃**（tap.rs:143-169）：慢插件会丢事件（ref 为无界 Stream 不丢）+ 订阅队列 512 溢出丢最旧。对依赖事件序列一致性的插件是语义差异。

### 4. 缺失文件/功能汇总
- 🔴 `mcp/index.ts:415` onSessionExpired 后台重连路径
- 🔴 插件代际激活/fallback（plugin.ts:100-180 无 Rust 对应）
- 🔴 `@opencode/plugin/effect` API 面（G-148② 显式不移植）
- 🟡 `PluginUpdate.changes()` 热流与 outdated/updating 双旗标全链
- 🟡 非 agent 域的 transform 失败禁用
- 🟡 启动期 npm 自动安装（改为离线解析 + CLI `plugin add`）
- 🟡 progress 行逐次发射（G-74 聚合单次）
- 注：mcp/instructions、codemode catalog/instructions、oauth 全流、elicitation、stdio、tap 域、ctx API 面均确认在 Rust 侧有行级对应实现。
