# ADR-0003: SDK 内嵌服务器形态由 CLI lifecycle 承担（standalone 子进程租约），非 SDK 侧独立 server

- 状态：accepted（记录既有决定；依据 PARITY-LOOP §11.3）
- 日期：2026-10-18
- 上游版本：2.0.24 (ref e7a34f09bf)
- 背景：v2 的 npm SDK 消费者需要"内嵌服务器"形态：SDK 不自己实现 server，而是由 CLI 侧 `Standalone.start()` 拉起一个私有 server 子进程并持有租约。Rust 移植沿用同一形态（`cli/src/services/standalone.ts` 的移植），SDK/嵌入方通过 authed endpoint 连接该子进程。停止语义必须遵守 2026-10 事故规则（只打向自有 Child 句柄的 PID）。
- 决定：SDK 的 embedded-server 形态由 CLI lifecycle 的 standalone service 承担：spawn 私有 `serve --stdio --port 0` 子进程（随机 32 字节 lease 密码强制经 `OPENCODE_PASSWORD` 子进程 env 注入，防用户 env 遮蔽），从 stdout 首行 `{"url":...}` 读取就绪；stdin 管道即所有权租约（EOF = server 退出）。停止顺序固定：stdin EOF → SIGTERM → 3 秒期限 → SIGKILL → reap；信号只打向自有 `Child` 句柄当场取的 PID，绝不来自存储状态。
- 后果：代价是每次内嵌启动一个子进程（ready-line 60s 超时、stdout 排空线程、`exited` 轮询 250ms），以及 CLI 二进制必须可自举（`self_command`）；收益是 SDK 侧零 server 实现、进程边界即租约边界（宿主死亡时 EOF 仍由 OS 送达，子进程自终）。
- 证据（path:line / 测试）：
  - `crates/oc-cli/src/services/standalone.rs:1-5`（模块文档：spawn `serve --stdio --port 0`、stdin 租约、EOF = 退出、TERM 3s 后 KILL）
  - `crates/oc-cli/src/services/standalone.rs:100-118`（`start()`：`self_command` + `serve --stdio --port 0` + `OPENCODE_PASSWORD` 随机 32 字节 + piped stdio）
  - `crates/oc-cli/src/services/standalone.rs:53-83`（`release()`：先 drop stdin（EOF 租约信号）→ SIGTERM → 3s `try_wait` 循环 → SIGKILL → `wait()`；事故规则注释：PID 取自自有 Child 句柄）
  - `crates/oc-cli/src/services/standalone.rs:126-148`（stdout 读取线程：首行满足就绪、其余排空防 EPIPE、EOF 置 `exited`）
  - `crates/oc-cli/src/services/standalone.rs:173-178`（`ready_line_parses` 测试）
  - ref 对照：`/opt/opencode-ref/packages/cli/src/services/standalone.ts`（v2 同名服务，`killSignal: SIGTERM` + `forceKillAfter: 3s`）
- 复审日期：Batch 2 结束
