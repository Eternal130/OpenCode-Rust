# ADR 索引（Architecture Decision Records）

parity 程序的**既有决定**记录（PARITY-LOOP §11.3 建立的清单）。模板与规则见 `docs/PARITY-LOOP.md` §11.3；每个 `accepted` 必须有对应 ADR（§13 第 14 条），新增 ADR（0006+）属于后续工作，按需建立。

| 编号 | 标题 | 状态 | 一句话 |
|---|---|---|---|
| [0001](./0001-plugin-process-isolation.md) | guest 插件以隔离 OS 进程运行 | accepted | bun guest 插件 = 受监管子进程，RPC/KV 跨 stdio 边界；停止只走自有 Child 句柄或 safekill/procutil，绝不按名字/模式杀进程 |
| [0002](./0002-system-player-audio.md) | 提示音播放走系统播放器 | accepted | 播放委托系统播放器 spawn（detached、stdio null），非 Linux 通用进程内音频栈；Linux 现含 vendored miniaudio 进程内路径（OPEN：措辞待裁决） |
| [0003](./0003-sdk-embedded-server.md) | SDK 内嵌服务器形态由 CLI lifecycle 承担 | accepted | standalone 服务 spawn `serve --stdio --port 0` 子进程，stdin 管道即租约：EOF → TERM → 3s → KILL |
| [0004](./0004-scope-desktop-web.md) | desktop/web（营销站）/gui-extensions 出范围 | accepted | `packages/desktop`、`packages/web`、`gui-extensions` 不进 parity 队列；Web UI（`packages/app`）仍在范围内（OPEN：README 措辞 release 前对齐） |
| [0005](./0005-hook-copy-semantics.md) | hook 跨界 copy-in/copy-out 语义 | accepted | guest hook 事件 JSON 副本进出子进程边界，等价标准是可观察等价（顺序 + mutation 形状），非内部表示位同一 |
