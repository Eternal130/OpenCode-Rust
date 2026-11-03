# 0018 — oc-tui 渲染面全量切换 opentui-rust（推翻混合架构裁决）

- 状态：accepted（2026-11-03，Phase B4 收口）
- 取代：FRAMEWORK-CONVERGENCE.md 的「混合架构 = 目标形态」裁决（已归档；本 ADR 为其正式推翻记录，见 `docs/PLAN-opentui-render-migration.md`）
- 范围：crates/oc-tui（+ oc-cli 的 mini 前端）；opentui-rust 侧为 feature `terminal-backend`（rev 4ff646d）

## 背景

oc-tui 的终端栈原为三层混合：自有组件树/画布 + ratatui（离屏 Buffer、CrosstermBackend 差分输出、`style` 词汇表）+ crossterm（raw mode、alt-screen、模式序列、事件线程）。旧裁决认为该混合形态即目标形态。Phase A（opentui-rust terminal-backend：TTY/termios、模式序列、能力探测、输入/输出泵、resize/teardown）与 Phase B（oc-tui 切换）证明全量切换成本可控且行为可钉，混合裁决作废。

## 决策

1. **GridEmitter a1 路线**：帧发射 = oc-tui 自有的 `GridEmitter`（`crates/oc-tui/src/grid_emitter.rs`）——opentui-core `emit_frame_sgr`（renderer.rs :2248-2386）的独立值移植，输入是自有 `render::Frame` 网格，输出经会话终端的 `output()` 写口、包 `?2026` 同步包裹（不按能力门控，随上游）。与 opentui 的 `bench_emit_frame_sgr` 差分测试钉字节平价。
2. **headless 输入泵**：输入 = opentui `feed_bytes` 之上的 `KeyPipeline`（`input_term.rs`，B3a）——fd 非阻塞读 + 解析 + 事件回填；不再有独立事件枚举线程，crossterm poll/read 全退。
3. **探针事件回填**：能力探测（DA1/DECRQM/CSI ?u/OSC11）答案随输入流回灌 `Capabilities` 事件，消费侧（kitty push、color_mode、theme）以事件到达为准。
4. **suspend = 重建**：挂起恢复走 `Terminal::teardown` + 同 TTY `Terminal::new` 重建（重跑 raw mode + setup burst），不做部分恢复。
5. **mini 薄缝**：mini split-footer 不移植 inline viewport/`insert_before` 引擎，改为薄缝——dock 经同一 `GridEmitter` 画在全屏网格底部（上方行恒为 blank，diff 永不触碰转录区），`insert_before` 以「擦 dock → 底行滚动 → CUP 写行」等价实现（对齐 ratatui 0.29 no-scrolling-regions 语义，注释引上游）；键入走同一 `KeyPipeline` 非阻塞路径；全部字节经同一 `Stdout` W（mini 单线程自持，主 app 的锁约定由构造保证）。行为准绳 = oc-cli `mini_pty.rs` + oc-tui mini 系列测试，断言未改。

## 登记的窄化与偏差（bounded deviations）

- **split-footer / ExternalOutputCommit 整族延后**：主 TUI 不消费 split-footer；renderer.ts:2339+ 的 makeRoom/ExternalOutputCommit 家族不移植（mini 走薄缝）。见 GAPS G-211。
- **panic hook 保留 oc-tui 线程门控版**：opentui 的 install_panic_hook 无条件注册；oc-tui 保留自有 hook（线程门控——worker 线程 panic 不拆终端），驱动 `Terminal::teardown`，不双注册。
- **OSC 8 无槽回收 + link-id 门**：帧词汇表不携带 link id，emit 的 OSC 8 arm 收敛为上游 0 路径；链接点击按 link-id 门（hit walk 持 URL）。
- **resize ≤50ms 轮询**：SIGWINCH 之外保底轮询 TIOCGWINSZ（≤50ms 周期），不依赖单一信号流。
- **kitty push 字节留 oc-tui**：`>u` push/pop 由 terminal_proto 探针门控后下发，opentui 侧 flags 恒 0（无 post-setup push 面）。
- **孤立 ESC 适配层 flush**：read chunk 以孤立 ESC 结尾时立即 flush（crossterm `input_available=false` 臂的平价），由 KeyPipeline 持有。
- **鼠标 feed 前拦截**：鼠标序列在键解析前路由（crossterm 顺序平价）。
- **mini 运行中 resize 不重排**（Gate B M3）：与退休 ratatui Inline 版平价（同名 dead_code 可证），非回归；Phase C 动 mini 时再接 resize。
- **resize 检测寄生输入循环 tick**（Gate B M5）：单帧构建耗时长的重帧场景下检测延迟可超 50ms；边缘场景，登记接受。

## 证据指针

- 差分测试：`crates/oc-tui/src/grid_emitter_tests.rs`（与 opentui `bench_emit_frame_sgr` 字节平价，含 cursor tail 两 policy）。
- goldens：`tools/parity/goldens/`（264 文件，B2/B4 零 diff）；mini 帧金样 `tools/parity/goldens/mini-frames/`。
- PTY 准绳：oc-tui `tests/vt_harness.rs` 族（vt100 网格比对）+ `tests/pty_daily.rs`（含 `mini_subagent_footer`）；oc-cli `tests/mini_pty.rs`（4 测试：splash/scrollback/exit、permission、CONFIG-T4 门、demo）。
- 测试日志：`.test-logs/workspace-*-laneB4-*.log`（default + pty 双 profile）。
- 依赖证明：`cargo tree -p oc-tui` 无 ratatui/crossterm（B4 验证）；`git grep "ratatui\|crossterm" -- '*Cargo.toml'` 零命中。

## 后果

- ratatui/crossterm 从 workspace 依赖图删除（Cargo.lock -223 行）；oc-tui 终端栈单源（opentui-core terminal-backend + oc-tui GridEmitter/input_term）。
- Phase C backlog（solid 表面收敛、TextTable/ImageRenderable、TextBuffer selection）不依赖本 ADR，按维护价值排队。
