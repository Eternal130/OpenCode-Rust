# PLAN — oc-tui 渲染面全量切换 opentui-rust(弃 ratatui/crossterm)

> 状态:active(2026-11-03 立项)。deepwork 任务:opentui-render-migration。
> 目标:终端 I/O、帧发射、输入解析全部由 opentui-rust 拥有;删除 ratatui/crossterm 依赖;行为对齐 ref(opencode v2.0.24 = @opentui 0.5.14)。
> 本计划取代「混合架构=目标形态」旧裁决(FRAMEWORK-CONVERGENCE.md,已归档);ADR 在 Phase B4 落地。

## 现状(2026-11-03 调研结论)

- oc-tui 管线:组件树 → opentui-solid 表面(SolidRenderer.flush→OptimizedBuffer)+ 手绘面(paint_placed→ratatui Buffer)→ 自有 Frame 网格 → terminal.draw 逐格拷 → CrosstermBackend diff → stdout。
- ratatui 实际用途仅三类:离屏 Buffer 画布、CrosstermBackend 差分输出、`style::{Color,Style,Modifier}` 类型词汇表。零 widget。
- crossterm 用途:raw mode、alt-screen、?1004/?2004、事件 poll/read 线程;鼠标模式/kitty push-pop/DECSCUSR/DA1/OSC11 探针/teardown 序列本就自管(mouse_modes.rs、terminal_proto.rs、theme.rs)。
- opentui-rust renderer 纯 headless:`emit_frame_sgr` 已产出完整 ANSI 字节流;`feed_bytes` 为输入接缝;键鼠/paste 解析件全套;**TTY/termios/模式/探测/resize/epilogue 整层缺失**(core 无系统依赖)。上游对应:TS 编排(renderer.ts)+ zig native(terminal.zig/renderer.zig/renderer-output.zig);termios/SIGWINCH 上游靠 Node 宿主,Rust 需自建(rustix)。

## Phase A — opentui-rust 终端后端(repo: /opt/opentui-rust;fixer lanes A1→A2→A3 串行;oracle gate)

feature-gated `terminal-backend`(仿 device-audio 先例;默认构建保持 headless 差分纯净)。

- **A1 骨架+TTY+模式+生命周期**:`terminal` 模块;TTY 抽象(termios raw/restore,rustix,unix-only + trait seam);模式序列写层(mouse ?1000/1002/1003/1006、paste ?2004、focus ?1004、kitty CSI>u/<u、modifyOtherKeys、?2026 同步包裹、光标显隐);setup(进 alt-screen + saveCursor + 模式下发)/ teardown epilogue(退屏/复模式/复光标)。移植源:terminal.zig:955-1058、renderer.zig:539-575/588-640;oc-tui teardown(app.rs:9646-9672)与 mouse_modes.rs 作逐字节交叉核对。单测引上游 file:line。
- **A2 能力+事件+泵**:探测 burst(xtversion/DA1/DECRQM/CSI?u/OSC11)→ 响应状态机 → Capabilities + 5s 超时(terminal.zig:347-399, 1247-1446;复用 parse_keypress.rs:552-630 谓词;theme.rs 探针经验);CliRenderEvent 增 Resize/Capabilities 变体;SIGWINCH resize 源→renderer.resize();输入泵(fd→feed_bytes);输出泵(sgr_stream→io::Write 缓冲,renderer-output.zig:167-233,同步版)。
- **A3 集成测试+收尾**:PTY 对(posix_openpt)字节级集成测试;scrollback teardown 最后一跳(退屏后写 paint_lines);OSC 8 下发(emit_frame_sgr 读 link id;差分场景 --refresh-oracle 复验);README/PORT-MATRIX 回写;push origin main。
- 有意裁剪(登记):inline viewport(split-footer/ExternalOutputCommit 整族)留 Phase B4 决策——最小 makeRoom 移植 vs mini 薄缝;palette/theme 检测消费面后排。

**Gate A**:oracle 评审(契约级对齐上游、API 形态、feature 门、风险)+ explorer 结构扫描并行。

## Phase B — opencode-rust 切换(repo: /opt/opencode-rust;fixer lanes B1→B2→B3→B4 串行;oracle gate)

- **B1 rev bump**:opentui 三 crate bump 至 Phase A 新 commit,开 terminal-backend feature;全 workspace nextest 基线绿。
- **B2 类型词汇替换(纯机械,先行降面)**:`ratatui::style::{Color,Style,Modifier}` → opentui 对应物(color::Rgba/TextAttributes/StyledText/SyntaxStyle);render/paint.rs 的 ratatui Buffer → 自有 render::frame::Frame 画布;oc-syntax 3 文件同步。goldens 钉零行为变化。
- **B3 主 TUI I/O 切换**:app.rs 拆 CrosstermBackend/Terminal/draw 桥 → 后端泵+输出泵;enable_raw_mode/execute! → 后端 setup/teardown;crossterm 事件线程 → 后端输入泵+CliRenderEvent(tokio mpsc 桥保留);theme 探针→Capabilities;color_mode.rs env 探测保留;mouse_modes/terminal_proto 替换为后端等价。PTY 测试(vt_harness,vt100 网格比对)为准绳。
- **B4 mini+清理**:mini(Inline viewport)按 Gate A 决策落地(最小 inline 或薄缝);删 ratatui/crossterm 依赖;写 ADR-000X(推翻混合裁决);GAPS/ROADMAP 登记;磁盘清理(worktree target)。

**Gate B**:oracle 评审 + explorer 结构扫描。

## Phase C — 增量收敛 backlog(不阻塞依赖移除,按维护价值排队)

表面逐个上 solid builder(仿 16 个 *_solid.rs 先例)+ opentui-rust tier-2:TextTableRenderable(markdown 表格,连带 G-177)、ImageRenderable(图片面)、TextBuffer selection/truncate/scissor(编辑器保真)、markdown streaming。

## 验证矩阵

| 变更 | 门 |
|---|---|
| opentui-rust 默认构建 | cargo nextest 全绿 + tools/diff 冒烟(涉 renderer 输出则 --refresh-oracle) |
| terminal-backend feature | 单测(file:line 引上游)+ PTY 对集成测试 |
| oc-tui 类型替换 | goldens(264 文件)零 diff |
| oc-tui I/O 切换 | PTY 27 测试(vt100 网格)+ nextest --workspace |
| 每 lane | dev-gate + tee .test-logs + 合并门 |
