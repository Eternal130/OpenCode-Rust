# P1-GATE 结论: 12 份 lane 报告质量高、证据链总体可信——5 条抽查全部核验成立，2 处跨 lane 矛盾已裁定（C6a 对 C4/C1 的两处"证伪"一对一错），无 P0，判定**有条件通过**（条件见末节）。

## 矛盾裁定

**裁定#1（orchestrator 已定，本次确认）**：`mcp.timeout` 文档级默认——C4 R-1 成立（rust `engine/mod.rs:3329` 附近 `mcp_servers_from_config` 只读 `mcp.servers`；ref `config/plugin/mcp.ts:44-59` 有 `{...timeout,...server.timeout}` 合并）；C6a 对比表中"C4 结论被证伪"的表述**不成立**——C6a 引 `client.rs:1262-1278` 消费的是 **per-server** `config.timeout`，与文档级 `mcp.timeout.*` 是两个层面。最终口径：per-server timeout 生效 ✅，文档级全局 timeout 静默失效 ❌（采用 C4 R-1）。

**裁定#2（本次新增）**：C1 R-1（坏工具拖垮整批注册）vs C6a 反驳——**C6a 反驳成立，C1 R-1 的外推部分撤销**。核验：`registration_error` 全仓仅 `tool/registry.rs:61,142` 使用（内建静态路径）；MCP 工具注册走 `mcp/registry.rs:1920-1928` 直接 `tools.register(ToolRegistration)`（infallible BTreeMap insert，`location.rs:561-564`），**不经过** `registry::register` 校验。故"一条坏 MCP 工具名使整批注册失败"不成立；C1 D-6 仅在内建静态工具路径为真（无实际影响）。此裁定同时意味着：rust 的 MCP 注册面**完全没有** ref 的 RegistrationError 校验（ref tool.ts:177-218 对 MCP 工具也校验）——超长/非法名 MCP 工具在 rust 会被静默注册。

**裁定#3**：C2a 与 C3 对 compaction/inbox/plan 的交叠结论**一致，非矛盾**：C2a D-13 与 C3 D-1/D-2 是同一发现的两面（plan 提醒不持久化 + 渗入压缩请求），行号互证（`model_request.rs:187-190` vs `:187-199`）。

**裁定#4**：C7b D-5 与 C1 D-7 为**重复发现**（归并处理）。

**裁定#5**：C4 R-7 与 C6b D-3——C6b 已主动复核并给出"功能闭环成立、粒度更粗"结论（`engine/mod.rs:770-790` 无条件刷新 + 无源早退），两报告**兼容**，采用 C6b 的闭环表述。

未发现其他实质矛盾。

## 归并表（重复/交叠发现）

| 归并项 | 出处 | 统一口径 |
|---|---|---|
| `snapshots:false` 无消费者 | C4 R-2 + C5b R-1 | C5b 提供 ref 证据链（config/plugin/snapshot.ts:15-19→snapshot.ts:83-87,115），C4 提供 rust 缺消费链；抽查确认 `configure` 仅测试调用。**成立** |
| websearch progress 时机 | C1 D-7 + C7b D-5 | rust `websearch.rs:765-771` 结果边界一次 vs ref 每次尝试前。成立 |
| plan 提醒不落库+渗入压缩 | C2a D-13 + C3 D-1/D-2 | 合为一条 |
| `mcp.timeout` | C4 R-1 + C6a | 裁定#1 |
| 注册失败通道 | C1 R-1/D-6 + C6a | 裁定#2 |
| wellknown 刷新触发粒度 | C4 R-7 + C6b D-3 | 裁定#5 |
| localeCompare/ICU 近似族 | C1 + C3 D-9 + C7b D-8 | 同族偏差：非 ASCII 排序不保证一致，低危 |
| 中断/终局 5s/60s 有界等待 | C2a D-10 + C2b D-5/D-15 | 同类"防呆上限"偏离族，合并跟踪 |
| 空 part/wire 微差 | C2a D-12 + C3 D-6 | wire 层微差族，低危 |

## 主风险登记册（按严重级排序；P0 无）

| ID | 域 | 摘要 | 双侧证据 | 状态 |
|---|---|---|---|---|
| G-01 | C8 | CodeMode 沙箱逃逸：仅换装 console/fetch/timers，Bun/process/`__rawFetch` 全可达 | host.js:2833-2856（2841 设 `__rawFetch`，2863 AsyncFunction）vs ref interpreter.ts 结构性无进程面 | **成立（已抽查）** |
| G-02 | C7a | 非持久 pty per-chunk lossy UTF-8 损坏多字节输出 | pty.rs:623-638 vs ref node-pty 解码层 | **成立（已抽查）** |
| G-03 | C2a | 中断后 hosted 工具多发 `tool.failed(result-missing)` | step.rs:557（未排除 stream_interrupted）vs step.ts:220 `Exit.isSuccess` | **成立（已抽查）** |
| G-04 | C4/C5b | `snapshots:false` 被接受但永不生效，快照恒写 | ref snapshot.ts:83-87,115 vs rust `configure` 仅 snapshot/tests.rs:168-170 | **成立（已抽查）** |
| G-05 | C8 | R-2+R-3：结构上限仅桥上生效 + token 级扫描可绕（`eval`/`new Function`） | host.js:104-139,2666-2668 vs ref limits.ts:15-21 | 成立（与 G-01 叠加） |
| G-06 | C6b | 默认模型无"首个可用 text 模型"回退 | resolver.rs:660-673 vs model-resolver.ts:402-417 | **成立（已抽查）** |
| G-07 | C6a | MCP 目录分页截断（单次 List* 不跟 nextCursor） | client.rs:855-971 vs ref SDK 自动分页（node_modules 不可见） | 需实测复核 |
| G-08 | C6a | 运行期 OAuth token 过期无 refresh/恢复 | client.rs:1498-1502,799-803 vs ref client.ts:149-151 | 成立 |
| G-09 | C7b | location 关闭后 RPC 域仍可调（无 rpc.close） | location.rs:978-992 vs location-lifecycle.ts:44-54 | 成立 |
| G-10 | C7a | persistent-pty 协议超集共用版本号 7 的互操作风险 | wire.rs 超集 vs ref v7 | 成立（建议版本协商） |
| G-11 | C5b | vcs 命令无 cfg 标志 + scope 放大至全仓 + status 统计恒 0 + "AM"映射错 | handlers/project.rs:1394,1832-1860 vs plugin/vcs/git.ts:63-99,152-172 | 成立 |
| G-12 | C2a | 事件面族：D-2 无 finish 判失败/D-5 弃用提示/D-7 service 恒 "opencode"/R-8 空 provider state | misc.rs:39, mod.rs:445, to_llm_message.rs:36-44 | 成立（低危群） |
| G-13 | C5b | location 越界静默全仓快照（D-1），与 G-04 叠加放大 | engine/mod.rs:1807 vs snapshot.ts:110-111 | 成立 |
| G-14 | C6a | 桥同步无互斥（初始 vs 去抖竞态）+ sessionID 元数据丢失（D-2） | registry.rs:1672,1831-1882 vs tool/mcp.ts:33,69,124 | 成立 |
| G-15 | C7a | 服务无 teardown（pty/shell 子进程与 waiter 无终态化） | pty.rs/shell.rs 无 Drop vs pty.ts:128-134 | 成立 |
| G-16 | C7b | mime 三副本不一致（read.rs PNG 4 字节签名）+ reload_all 吞失败 | read.rs:126-151, location_services.rs:316-326 | 成立（低危） |
| G-17 | C1 | question 布尔回案映射 + 输出 schema 全豁免（G-230） | question.rs:160-167, registry.rs:220-237 | 成立（低危、部分有意） |
| G-18 | C2b | 60s 放弃 drain + 10s 强删窗口写入已删聚合（R-2）；boot 悬置 `list_suspended_before`（R-3） | mod.rs:59-82, store.rs:398-523 | 需复核 boot.rs:1241 附近 |
| G-19 | C4 | `lsp` 键惰性、全局 AGENTS.md 读失败过度降级 | instruction.rs:104-107 | 成立（低危） |
| G-20 | C5a/C3/C6b | 低危群：replay 不解码畸形负载、canonical_json 浮点、scrub 不中止在途交换、copilot bearer 取 refresh 待交叉确认 | 各报告 R 列 | 成立/留档 |

## 补审建议（按优先级）

1. **必补**：C5b 缺口——ref `git.ts` clone/fetch/checkout/reset/worktree 族（~612-758）在 rust 全仓无对应（lane 自认 grep 无 fetchRemotes/resetHard）。需定位 worktree sync 逻辑去向；若真缺失则 worktree 依赖远端同步的行为整体缺位。建议补审 `handlers/project.rs` worktree 段 630+ + ref git.ts 后半。
2. **必查**：C2b R-3 悬置点——`boot.rs:1241` 附近是否调用 `list_suspended_before`。
3. **P1**：C7a 未读 `persistent_pty/frontend.rs`（583L）+ rust 独有 `read` 扩展 op（与 G-10 相关）。
4. **P1**：C6b 结构审读四件（providers.rs/providers_enterprise.rs/console.rs/facade.rs）——抽审 enterprise 鉴权/计费路径与 facade 的 `${VAR}` baseURL 替换；C6b R-4（copilot bearer 取 refresh）应一并交叉确认。
5. **P1**：C3 残留——variant.ts 其余 ~40 家族 handler 机械 diff 常量表 + 抽 5 家族互校；`optimize/*.txt` 与 ref `system-prompt/*.txt` 字节 diff。
6. **P2**：C2a R-9——oc-ai websocket SessionChannels 生命周期参数（已排入 A2 lane）。
7. **P2**：C4 残留盲区（agent/command/source/skill/provider 插件、v2_migration 实现体、project.rs 701-2124）——接受现状，登记。
8. **P2**：C6a R-1 分页——真实分页 MCP 服务器对测或读 rmcp list 变体源码。
9. 接受现状：C8 openapi/fromSpec 未移植（产品内无消费点）、C7a win32 ConPTY 姿态——已知缺口。

## 抽查核验结果（5 条 + 1 裁定，全部亲读源码）

1. **C2a R-1**：`step.rs:482` 有独立 `stream_interrupted` 标志，而 `step.rs:557` 的 RESULT_MISSING 门只查 `stream_error.is_none() && !provider_failed`——中断时门通过，hosted 工具被误落失败。**✅**
2. **C4 R-2/C5b R-1**：全仓 grep `GitSnapshotStore::configure` 调用点仅 `snapshot/tests.rs:168-170`；engine 侧只 configure compaction/tool_output。**✅**
3. **C8 R-1**：host.js:2833-2856 仅换装三面；2841 设 `globalThis.__rawFetch`；2863 AsyncFunction 在 guest 全局执行。**✅**
4. **C6b R-1**：`resolver.rs:661-673` 无 available 回退；ref `model-resolver.ts:402-417` 明确回退链。**✅**
5. **C7a R-1**：pty.rs:623-638 逐 8192B chunk lossy 解码。**✅**
6. **裁定#2 附加核验**：`registration_error` 仅 tool/registry.rs:61/142 引用；MCP 注册直插无校验。**✅**

## 门判定：**有条件通过**

放行条件：
1. **裁定落稿**：最终报告按裁定#1（mcp.timeout 采用 C4）与裁定#2（撤销 C1 R-1 外推、改记"rust MCP 注册面无名称校验"）修订对应 lane 结论。
2. **必补两项小成本审查**：C2b R-3 的 boot.rs 悬置点核实；C5b 声明的 ref git.ts clone/fetch/reset/worktree 族去向定位（若确认缺失需升级为独立发现）。
3. **P1 修复跟踪**：G-01（至少先删 `__rawFetch` 残留）、G-02（pty 字节缓冲化）、G-03（补 `&& !stream_interrupted`）、G-04（接通 snapshots 开关）、G-06（默认模型回退）进入修复队列。
4. G-07（MCP 分页）放行后以真实分页服务器对测闭环。

## 放行条件执行结果（orchestrator 核验，门转为**无条件通过**）

1. **条件#2a（boot.rs 悬置点）已解决**：`oc-server/src/boot.rs:670-686` `recovery_sweep` 使用带守卫的 `list_suspended_before(boot_started_ms)`（RUNTIME-SWEEP-RACE 注释完整论证：只恢复 boot 前的 claim，当前/并发 boot 的活会话不会被误恢复）。`jobs.rs:1021/1032` 的无守卫 `list_suspended()` 属 `resume_suspended_sessions`，grep 无生产调用方（仅定义）。**C2b R-3 关闭：boot 路径安全**。
2. **条件#2b（git.ts clone/fetch/reset/worktree 族去向）已定位，非缺失**：ref `git.ts` 的 `Git.worktree.{create,remove,list}` 由 rust `oc-server/handlers/project.rs:330+`（`git worktree add --detach`，C7b 已审对齐）承载；`Git.sync.{fetchRemoteBranch,checkoutRemoteBranch,resetHard}` 在 ref 侧仅被 `repository-cache.ts:204`（reference 服务的仓库缓存）消费，rust 对应物为 `oc-core/src/reference/repository_cache.rs:279`（`fetch --all --prune` 注释自证移植）。**C5b 声明的"缺口"实为位置转移，不成立为缺失**；残留：repository_cache.rs 在 C7b 仅为按需抽查，最终报告按"结构级核验"登记。
3. 条件#1（裁定落稿）与#3/#4（修复跟踪/分页对测）移交最终汇总阶段执行。
