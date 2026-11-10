# 第十九轮复核 V-A（核心/注册表/总线）— 2026-11-09

> 对 17 通道审查（docs/audit/L1-L17）产出候选逐项现行源码双侧复核，与登记册 G-1..G-290 去重。结论：11/11 NEW-CONFIRM。

1. **[🔴] 工具注册表按 bare name 键控 → MCP 跨服务器同名工具互相覆盖**
   - rust：`oc-core/src/location.rs:558-559`（`tools.lock().insert(tool.name.clone(), …)` 键=bare name）、`:624-625`（executor 同键）；MCP 桥 `mcp/registry.rs:1879-1887`（`ToolRegistration::new(name=bare tool.name)` + `register_executor(name…)`，:1894-1904）
   - ref：`packages/core/src/tool.ts:182-184`（`const id = effectiveName(tool); editor.tools.set(id, …)`）
   - 理由：两个 MCP server 各暴露 `search` 时，注册表与 executor 表仅存一份（后者覆盖前者），且 MCP 工具 bare name 还可覆盖同名 builtin（ref 因 effectiveName 前缀天然隔离）；派发面 `model_request.rs:1024-1038` 靠 snapshot 的 effective→bare 映射，源表已丢行，路由必错。G-242 修的是执行路径（PipelineExecutor/permission），不覆盖注册键。
   - 修法：`ToolRegistryState::register/register_executor/remove/executor` 统一改 `effective_name(namespace, &name)` 为键。

2. **[🟡] tool_input_repair 查表键失配**：rust `tool/tool_input_repair.rs:46` 用 bare 键表查 `event.tool`（=effectiveName，registry.rs:157/1897）→ MCP 与 `opencode_*` 工具 repair 静默失效；ref `plugin/tool-input-repair.ts:29` 同键必命中。随 #1 改键自动对齐。

3. **[🟡] Bus durable 发布 notify 在聚合锁外**：rust `bus.rs:697-708`（drop(guard)→drop(lock)→notify），replay :1341-1352、publish_all :858 同型；ref `bus.ts:469-477` 锁内 notify。同聚合并发发布订阅者可见 seq 乱序；模块头自述与实现不符。G-207 只修时间戳。修法：notify 移回 guard 存活区间。

4. **[🟡] 插件热重载先卸后装**：rust `supervisor.rs:1942-1951` 先 clear+unload 旧代→`:1955-1966` load 新代→失败仅 eprintln；ref `supervisor.ts:70-86` 新代成功才替换（"keeps its place"）。G-222 只修触发面。修法：先 load 成功后再切换。

5. **[🟡] MCP 后台 list-changed 观察 session 过期不重连**：rust `mcp/registry.rs:909-957` watch 无 session-expired 腿（typed 判定 client.rs:155/499 仅前台消费）；ref `mcp/index.ts:415` onSessionExpired→fork(recover)。G-223 修前台判型。修法：watch 挂 stop+start（replay_after_recovery 同款）。

6. **[🟡] 插件 RPC events.emit 宿主通路缺失**：rust `host.js:1462-1494` ctx.rpc.register 仅返回 `{dispose}`，hostRequest 方法表无 rpc.event*；客户端订阅侧在（oc-client/rpc_typed.rs:222-247）但无发布方。ref `rpc.ts:91-103`。第十七轮勘误 #6 撤销的是 SSE/bus 侧指控，不同面；G-2/G-64/G-147/G-148 均不含。修法：host.js 补 events.emit → 宿主 bus ephemeral。

7. **[🟡] 插件代际激活模型缺失**：rust supervisor 全文件无 revision 概念（generation 仅计数器 :1952，reload 全量卸装，无前缀 diff/失败修订不重试/fallback）；ref `plugin.ts:100-180` + supervisor.ts:87-90。与 #4 同文件不同面。修法：随 #4 一并移植 (id,revision) diff + fallback；或先登记 bounded。

8. **[🟡] project worktree.resolved 行插入与事件非原子**：rust `project.rs:1354-1364`（存量行短路→先 insert）→`:1381-1388`（后 publish）；ref `project.ts:184-199` insert 在 durable 事件 commit 回调内。崩溃窗口后事件永不补发。修法：publish 传 commit 回调（rust bus 已支持 PublishOptions::commit，bus.rs:684）。

9. **[🟢] patch `*** Move to:` 新目录不建父目录**：rust `patch.rs:430-433` write_file（G-1 条目自述不建父目录）；ref `environment/local.ts:50-55` files.write 隐式 mkdir -p（G-73 勘误确认 ref 建父目录）。"ref 成功、rust 失败"的 parity 断裂从未登记。修法：move-write 腿先 create_dir_all；或明确登记有界决策。

10. **[🟡] read 工具 AGENTS.md 发现绕过 Files seam**：rust `read.rs:931-941` std::path::parent()+is_file()（宿主 FS）；ref `read.ts:89-100` fs.resolve/up（Environment seam）。G-1 的"read 已迁 seam"只覆盖读腿。修法：发现腿改走 Files seam。

11. **[🟢] normalizeImages 与 truncate 顺序颠倒**：rust `step.rs:716-747` 先 truncate(:718) 后 normalize(:744)（:740-742 注释断言错误）；ref normalize 在工具内先做（tool.ts:150）、truncate 后（step.ts:119-124）。图片参与预算时截断点漂移。修法：换序并修注释。

汇总：NEW-CONFIRM 11（🔴×1、🟡×8、🟢×2）。#1+#2 同根（注册键）同 lane 修；#4+#7 同文件合并收口。
