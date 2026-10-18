# ADR-0001: guest 插件以隔离 OS 进程运行（bun 子进程 + stdio RPC/KV 边界 + safekill 停止语义）

- 状态：accepted（记录既有决定；依据 PARITY-LOOP §11.3）
- 日期：2026-10-18
- 上游版本：2.0.24 (ref e7a34f09bf)
- 背景：v2 的插件是 TS/ESM 模块，与宿主同运行时（bun/Node，进程内 Effect 链）。Rust 宿主无法进程内执行 JS guest，`/tmp/opencode/spike-bun` 原型验证后选定 option B：由受监管的 `bun host.js` 子进程承载 guest。插件的 hooks、KV、MCP、RPC 因此必须跨进程边界。同时，2026-10 的生产事故（PID 复用/按名字杀进程误杀系统服务）确立了进程停止的硬约束。
- 决定：guest 插件一律以独立 OS 进程（`bun host.js`，piped stdio，`kill_on_drop`）运行，绝不进程内执行；host↔guest 以双向 NDJSON JSON-RPC 通信，KV（`storage.*`）与 hook dispatch（`hook.dispatch`）都经 plugin host 的桥接跨越边界；进程停止只走两条安全路径——自有 `Child` 句柄上的 `kill()`（supervisor shutdown / `kill_on_drop`），或 oc-server 生命周期停止的 safekill/procutil 语义（pidfd 定点投递 + `(boot_id, start_time)` 代际绑定 + 外来记录 cmdline 身份校验，TERM→KILL 升级），绝不按进程名/模式 kill。
- 后果：收益是 guest 崩溃/挂死只损失自己的 dispatch（超时上限 2s 注册 / 5s 派发），语言边界天然隔离，宿主不被 JS 运行时污染。代价是每次调用的序列化与超时开销、必须结构性遵守死锁协议（IO reader 不阻塞、写通道不持有等待、全部调用超时上限）、崩溃归因 + 指数退避重启 + 插件级禁用的监管复杂度，以及 KV 目前落在 sandbox 内按插件分文件（`<sandbox>/plugin-storage/<id>.json`，值 256 KiB / 1 万 key 上限）。KV 跨进程重启持久化的生产接线仍是 PLUGIN 簇（C2）的未完事项，不在本 ADR 决定。
- 证据（path:line / 测试）：
  - `crates/oc-plugin-host/src/lib.rs:1-27`（bun 子进程 JS plugin host、双向 NDJSON JSON-RPC、facade 与监管总述）
  - `crates/oc-plugin-host/src/supervisor.rs:1319-1341`（`spawn_process`：`Command::new(&config.bun)` + `host.js` + piped stdio + `kill_on_drop(true)`）
  - `crates/oc-plugin-host/src/supervisor.rs:1-24`（supervisor 职责与死锁协议）、`supervisor.rs:1418-1431`（shutdown → supervise loop 对自有 Child 句柄 `child.kill().await`）
  - `crates/oc-plugin-host/src/supervisor.rs:304-306` 与 `supervisor.rs:1541-1563`（`storage.get/set/remove/scan` 跨边界的按插件 KV；上限与文件布局）
  - `crates/oc-plugin-host/src/context/mod.rs:112-133`（`Bridge::dispatch_hook`：hook 事件以 JSON 经 `hook.dispatch` RPC 跨界并取回）
  - `crates/oc-server/src/safekill.rs:1-32`、`safekill.rs:161`（`safe_signal`：代际绑定/身份校验/pidfd，事故史与"宁可少杀"原则）
  - `crates/oc-server/src/lifecycle.rs:1071-1110`（`terminate()`：safekill TERM→KILL 升级；stale 记录删除且绝不 signalling）
  - `crates/oc-cli/tests/run_e2e.rs:607`（safekill foreign-pid 路径的 e2e 断言：never signalled）
  - ref 对照：v2 插件宿主同运行时进程内执行（`/opt/opencode-ref/packages/core/src/plugin/hooks.ts` 的进程内 Effect 链）
- 复审日期：Batch 2 结束
