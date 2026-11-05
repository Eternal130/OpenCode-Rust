# 0019 — oc-tui 渲染管线收敛至 opentui-rust 单源（推翻 ADR-0018 的 a1/薄缝裁决）

- 状态：in progress（2026-11-05，Gate D2 批准起草；终裁于 D4 收官时定稿）
- 取代：ADR-0018 决策 1（GridEmitter a1 路线）与决策 5（mini 薄缝）；ADR-0018 其余登记偏差全文携带至本 ADR 偏差账（不得静默丢弃）
- 范围：crates/oc-tui（+ oc-cli mini 前端）与 opentui-rust 渲染面的接线关系

## 背景

ADR-0018（2026-11-03）在 ratatui/crossterm 移除时裁定：帧发射=oc-tui 自有 GridEmitter（opentui emit_frame_sgr 的独立值移植）、mini 走薄缝、split-footer 引擎整族延后（G-211）。用户后续指令升级目标为 TUI 全量收敛（体验=ref、架构对齐、TUI 功能全部由 opentui-rust 承担、超集移除），Phase D2 据此落地管线收敛。

## 决策与现状（D2 已落地部分）

1. **渲染循环所有权归活 CliRenderer**（D2.1b，3615e28a）：泵的 draw 门经 `request_render`+`settle(1)` 调度；GridEmitter 暂为唯一字节发射器（阴影路径，字节逐不变由构造保证）。
2. **单一共享树 + 全缓冲合成缝**（D2.2，Gate D0/D2 间 oracle attempt-2 裁决）：chrome/transcript/composer/footer/dialogs/overlays 六族以绝对定位 wrapper 子树上树（子序=z）；`paste_frame` 函数与全部分支已删除（全仓代码引用 0）；入 Frame 的唯一缝=一次全缓冲合成（洞跳过/缝上卸载/Caret/Swap/Transparent/DimBg stamps）。
3. **mini 走真 split-footer 引擎**（D2.3，ae91ecb8）：`CliRenderer{SplitFooter, footer_height=4, CaptureStdout}`；`write_to_scrollback`→`flush_pending_split_commits`；DECSTBM 转录区；dock 底部锚定；insert_before 模拟已删。G-181 四缺口闭。
4. **GridEmitter 退役不变量**：Frame 零消费者时退役。D3-5 后 diff_viewer 族已入树；**D3-6 已清账手绘尾族**（证据见 `.slim/deepwork/tui-full-convergence/topic-d36-lane.md`）：
   - `marquee`/`masked_text`/`tab_pulse` 是纯计算库（零 Frame 写入），消费方均为已入树的 solid 面；`marquee` 已并入其唯一消费家族（tabs，`session_tabs_solid/marquee.rs`），另两者因多族共享保留为顶层原语。
   - `update` 是面模型：home banner 与 DialogUpdate 分别由 `home_solid`/dialog 族在树内绘制；唯一 Frame 生产者为 render-crash 兜底 `error_component_frame`（**合理保留**，非树面）。
   - `tool_rows` 兼两职：`DEFAULT_FOREGROUND`/`fill_style` 是活面共享词汇；其 fixture/golden 帧绘制器无生产调用点，已文档声明为 fixture-only。
   - 因此 D4 的 Frame 消费者 = 主 TUI 的单一合成缝/路由装配、mini split-footer 路径、`render/{frame,paint}` + `grid_emitter`，以及各 golden harness 双生（`mounted_surface_frame` 等）与 crash 兜底；逐项清单见 D3-6 报告。
5. **footer 内容 API 开题**：mini dock 内容现仍经 GridEmitter 单一帧写口绘制——opentui 仅暴露 `footer_height` 几何预留、无 footer renderable API。D4 以 ref 证据裁决（ref 的 footer 内容若骑 opentui renderable 树，则此为契约级移植缺口，按家法补引用移植；不预置投机移植）。

## 偏差账（逐项携带；D2.3 已关两项）

- **已关**（D2.3 重裁）：split-footer/ExternalOutputCommit 整族延后（G-211 薄缝）；mini 运行中 resize 不重排（M3）。
- **保留**（ADR-0018 原文携带）：resize ≤50ms 轮询语义；resize 检测寄生输入循环 tick（M5，上界=run-loop tick）；panic hook 线程门控版；OSC 8 无槽回收+link-id 门；kitty push 字节留 oc-tui；孤立 ESC 适配层 flush；鼠标 feed 前拦截。
- **新增**（D2.2 登记）：dialog scrim=post-seam 带行走（非树节点；洞格手绘区不经缓冲）；console/reconnecting 与 dialog 带行走共见时的 z 微偏离（方向登记，共见瞬态）；toast 右轨宽字簇裁剪单格 bg 微偏差；pill/migration 重座 8/9（修 dialogs 偏离，合 ref zIndex）。
- **运行机制登记**（D2.2）：reconnecting scrim 以 DimBg 缝章实现树节点裁决（OptimizedBuffer 固定黑混合底使字面 in-buffer alpha box 不可行；字节等价单测钉）。

## 证据指针

- 差分/门禁：`.test-logs/oc-tui-*-laneD22*/D23*`（双 profile 全绿、goldens 零 diff、PARITY_UPDATE 全程未触发）
- 架构裁决：`.slim/deepwork/tui-full-convergence/topic-d22-arch-oracle.md`（attempt 2 权威）、`topic-gateD2-oracle.md`
- 六族/mini 台账：`topic-d22-lanes.md`、`topic-d23-lane.md`
- D3-6 尾族裁决 + Frame 消费者终局清单：`topic-d36-lane.md`
