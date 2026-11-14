# TUI-FULLSCREEN — TUI 整屏差分（通道 8）

同一 `tui-diff-scenario/1` 场景（tools/diff/corpus/tui/SCHEMA.md）喂双侧 TUI 实现，
静止帧导出为 `tui-diff-grid/1` 稠密网格后归一化比对。判门：exact /
structural（G-368 RGB ≤8 容差，仍是分歧）/ diverge；CAUSES 已知因定性（本表），
adjudication 属 T9——未命中 CAUSES 的一切非 exact 均未裁决 → exit 2。

> 复现：python3 tools/diff/tui_fullscreen.py --write docs/audit/diff/TUI-FULLSCREEN.md

- rust 侧: `cargo nextest -p oc-tui --test tui_fullscreen_diff_driver`（env 门控
  TUI_DIFF_MANIFEST/TUI_DIFF_OUT，dev-gate 身份）。驱动面:
  crates/oc-tui/tests/tui_fullscreen_diff_driver.rs:1
- ref 侧: `bun --preload @opentui/solid/scripts/preload.js
  tools/diff/drivers/tui_fullscreen_ref.ts`（staged 到 STAGE，DIFF_REF_ROOT
  指向 ref 树，只读）。驱动面: tools/diff/drivers/tui_fullscreen_ref.ts:1
- 编排器: tools/diff/tui_fullscreen.py:1（manifest/staging/infra 判门/比对/本报告）

归一化声明: time* 钩子字段剥离（captured_at/driver_time/timestamp/elapsed）；
cells 稠密化，None/缺省子字段补规范默认（ch=' '、attrs 排序去重、fg/bg=[-1,-1,-1]）；
比较字段序 ch→attrs→fg→bg；RGB 每通道 |Δ|≤8 为 structural（G-368，仍计分歧）；
哨兵负值 vs 具体 RGB 一律 diverge；request_census 不参与比对（双侧 boot 请求模式
已知不对称，信息性记录）；stub_gap 检测到即 infra 红（SCHEMA 规则）。

## 计数

| 总量 | EXACT | STRUCTURAL | CAUSES 命中 | 未裁决 |
|---|---|---|---|---|
| 99 | 81 | 0 | 12 | 6 |

（verdict 分布：exact 81 / structural 0 / diverge 18；
gate 见编排器输出，未裁决>0 ⇒ exit 2）

## 分歧明细

| case | verdict | diff cells | first_diff | ref size | rust size | ref gaps | rust gaps | ref paths | rust paths |
|---|---|---|---|---|---|---|---|---|---|
| anim-spinner-running | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 28 | 17 |
| anim-spinner-running@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 28 | 17 |
| anim-spinner-running@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 28 | 17 |
| anim-stream-delta | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 28 | 17 |
| anim-stream-delta@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 28 | 17 |
| anim-stream-delta@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 28 | 17 |
| anim-stream-multi-part | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 28 | 17 |
| anim-stream-multi-part@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 28 | 17 |
| anim-stream-multi-part@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 28 | 17 |
| anim-toast-window | diverge | 148 | r1c27 ch ref='┃' rust=' ' | 80x24 | 80x24 | 0 | 0 | 28 | 17 |
| anim-toast-window@120x30 | diverge | 148 | r1c67 ch ref='┃' rust=' ' | 120x30 | 120x30 | 0 | 0 | 28 | 17 |
| anim-toast-window@320x80 | diverge | 148 | r1c267 ch ref='┃' rust=' ' | 320x80 | 320x80 | 0 | 0 | 28 | 17 |
| anim-tool-progress | diverge | 60 | r7c5 ch ref='⋯' rust='⠋' | 80x24 | 80x24 | 0 | 0 | 28 | 17 |
| anim-tool-progress@120x30 | diverge | 60 | r7c5 ch ref='⋯' rust='⠋' | 120x30 | 120x30 | 0 | 0 | 28 | 17 |
| anim-tool-progress@320x80 | diverge | 60 | r7c5 ch ref='⋯' rust='⠋' | 320x80 | 320x80 | 0 | 0 | 28 | 17 |
| command-palette | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| command-palette@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| command-palette@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| dialog-open-recent | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| dialog-open-recent@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| dialog-open-recent@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| dialog-status | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| dialog-status@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| dialog-status@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| fetch-503-model | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 20 | 14 |
| fetch-503-model@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 20 | 14 |
| fetch-503-model@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 20 | 14 |
| home-empty | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 20 | 14 |
| home-empty@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 20 | 14 |
| home-empty@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 20 | 14 |
| home-multi-session | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| home-multi-session@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| home-multi-session@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| models-dialog | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| models-dialog@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| models-dialog@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| plugin-boot-combo | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 20 | 16 |
| plugin-boot-combo@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 20 | 16 |
| plugin-boot-combo@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 20 | 16 |
| plugin-boot-idle | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 20 | 16 |
| plugin-boot-idle@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 20 | 16 |
| plugin-boot-idle@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 20 | 16 |
| plugin-boot-superpowers | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 20 | 14 |
| plugin-boot-superpowers@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 20 | 14 |
| plugin-boot-superpowers@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 20 | 14 |
| plugin-fallback | diverge | 560 | r1c16 ch ref=' ' rust='┃' | 80x24 | 80x24 | 0 | 0 | 20 | 14 |
| plugin-fallback@120x30 | diverge | 780 | r1c56 ch ref=' ' rust='┃' | 120x30 | 120x30 | 0 | 0 | 20 | 14 |
| plugin-fallback@320x80 | diverge | 780 | r1c256 ch ref=' ' rust='┃' | 320x80 | 320x80 | 0 | 0 | 20 | 14 |
| plugin-panel-open | diverge | 58 | r11c11 bg ref=[20, 20, 20] rust=[250, 178, 131] | 80x24 | 80x24 | 0 | 0 | 27 | 19 |
| plugin-panel-open@120x30 | diverge | 58 | r13c31 bg ref=[20, 20, 20] rust=[250, 178, 131] | 120x30 | 120x30 | 0 | 0 | 27 | 19 |
| plugin-panel-open@320x80 | diverge | 217 | r3c280 ch ref=' ' rust='▼' | 320x80 | 320x80 | 0 | 0 | 27 | 19 |
| session-empty | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| session-empty@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| session-empty@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| session-markdown | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| session-markdown@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| session-markdown@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| session-multi-turn | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| session-multi-turn@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| session-multi-turn@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| session-reasoning | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| session-reasoning@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| session-reasoning@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| session-scroll | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| session-scroll@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| session-scroll@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| session-step-failed | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| session-step-failed@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| session-step-failed@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| session-stream-idle | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| session-stream-idle@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| session-stream-idle@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| session-tabs | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| session-tabs@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| session-tabs@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| session-text | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| session-text@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| session-text@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| session-tool-failed | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| session-tool-failed@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| session-tool-failed@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| session-tool-success | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| session-tool-success@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| session-tool-success@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| session-wrap | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| session-wrap@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| session-wrap@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |
| sidebar-toggle | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 17 |
| sidebar-toggle@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 17 |
| sidebar-toggle@320x80 | exact | 0 | — | 320x80 | 320x80 | 0 | 0 | 27 | 17 |

### first_diff 明细

- `anim-toast-window` [diverge] row=1 col=27 field=ch ref='┃' rust=' ' cells_diff_count=148 — CAUSES: G-424 rust toast 动作 affordance 恒 'x'（ref '› {label}' + 宽面板）
- `anim-toast-window@120x30` [diverge] row=1 col=67 field=ch ref='┃' rust=' ' cells_diff_count=148 — CAUSES: G-424 rust toast 动作 affordance 恒 'x'（ref '› {label}' + 宽面板）
- `anim-toast-window@320x80` [diverge] row=1 col=267 field=ch ref='┃' rust=' ' cells_diff_count=148 — CAUSES: G-424 rust toast 动作 affordance 恒 'x'（ref '› {label}' + 宽面板）
- `anim-tool-progress` [diverge] row=7 col=5 field=ch ref='⋯' rust='⠋' cells_diff_count=60 — CAUSES: A2 ref 动画门控夹具伪影（运行中工具行 glyph：rust ⠋ 时钟驱动 vs ref ⋯ 静态）
- `anim-tool-progress@120x30` [diverge] row=7 col=5 field=ch ref='⋯' rust='⠋' cells_diff_count=60 — CAUSES: A2 ref 动画门控夹具伪影（运行中工具行 glyph：rust ⠋ 时钟驱动 vs ref ⋯ 静态）
- `anim-tool-progress@320x80` [diverge] row=7 col=5 field=ch ref='⋯' rust='⠋' cells_diff_count=60 — CAUSES: A2 ref 动画门控夹具伪影（运行中工具行 glyph：rust ⠋ 时钟驱动 vs ref ⋯ 静态）
- `plugin-fallback` [diverge] row=1 col=16 field=ch ref=' ' rust='┃' cells_diff_count=560 — CAUSES: G-425 插件失败态管理屏不列失败入口（/plugins 对话框源表缺失）
- `plugin-fallback@120x30` [diverge] row=1 col=56 field=ch ref=' ' rust='┃' cells_diff_count=780 — CAUSES: G-425 插件失败态管理屏不列失败入口（/plugins 对话框源表缺失）
- `plugin-fallback@320x80` [diverge] row=1 col=256 field=ch ref=' ' rust='┃' cells_diff_count=780 — CAUSES: G-425 插件失败态管理屏不列失败入口（/plugins 对话框源表缺失）
- `plugin-panel-open` [diverge] row=11 col=11 field=bg ref=[20, 20, 20] rust=[250, 178, 131] cells_diff_count=58 — CAUSES: B2 插件注册投影（rust 统一进程 server-entry 装载 vs ref TUI 仅 tui 入口面）
- `plugin-panel-open@120x30` [diverge] row=13 col=31 field=bg ref=[20, 20, 20] rust=[250, 178, 131] cells_diff_count=58 — CAUSES: B2 插件注册投影（rust 统一进程 server-entry 装载 vs ref TUI 仅 tui 入口面）
- `plugin-panel-open@320x80` [diverge] row=3 col=280 field=ch ref=' ' rust='▼' cells_diff_count=217 — CAUSES: B2 插件注册投影（rust 统一进程 server-entry 装载 vs ref TUI 仅 tui 入口面）

## CAUSES 定性

- **G-368 量化色（oklab 最近邻量化差）** — 命中 0 case
  - 仅 RGB 差且每通道 ≤RGB_TOLERANCE(8)、char/attrs 全等：双侧滚动区主题各经一次 oklab 感知最近邻量化（同一算法、不同浮点环境），量化格点选择可相邻一格——结构性色彩差，非语义差
  - ref packages/tui/src/mini/theme.ts:81-105（oklab/nearestIndexed），经 mini/theme.ts:117-125 V1 序列化恢复后量化
  - rust crates/oc-tui/src/mini/theme.rs:114-192（oklab 量化 nearest_indexed/scrollback_color，1:1 移植）
- **G-128 midframe 簇（spring/tween 未移植——中间拍属性位置差，末拍收敛）** — 命中 0 case
  - anim 场景的帧序列差：首拍（静息帧）与末拍逐格一致、仅中间拍出现ch/attrs 属性差——ref 弹簧/补间（ui/animation.ts advanceSpring）过渡的中间帧位置 vs rust 已到位（无过渡引擎，终态直渲染）的已立项过渡差；末拍收敛 ⇒ 登记不拦；末拍也差的不是 midframe 族，不命中本簇（拦，T9）
  - ref packages/tui/src/ui/animation.ts:1-60（advanceSpring，未移植）
  - rust crates/oc-tui/src/ 全树无 spring/tween（G-128 登记面）；rust 渲染为 (store, clock) 纯函数（A1，store.rs epoch/clock 接线）
- **G-49/G-368 节奏族（中间拍量化色差，末拍收敛）** — 命中 0 case
  - anim 场景的帧序列差：首拍与末拍一致、仅中间拍 structural（RGB ≤8、char/attrs 全等）——过渡帧的 oklab 量化格点相邻差（G-368 同因）或已登记的 repaint 节奏差（G-49，帧率文档化决策）在中间拍的采样差；末拍收敛 ⇒ 登记不拦
  - ref packages/tui/src/mini/theme.ts:81-105（量化）+ 组件 rAF 帧节奏
  - rust crates/oc-tui/src/app.rs:10138-10166（100ms 泵粒度，SCHEMA ANIM_TICK_MS 对齐）
- **G-423 多 text part 内联拼接渲染差（anim-stream-multi-part）** — 命中 0 case
  - anim-stream-multi-part 场景：同一 assistant 消息的两个 text part （ordinal 0/1）交错流式 delta——ref 将两 part 的文本内联拼接到同一行（'Alpha Beta part'），且双 text.ended 后仅显示 part 1 终文；rust 按分段段落渲染（两个独立块，终态两段完整）。A2 帧序列维度新发现的渲染语义差，登记（G-423），不拦
  - ref packages/tui/src/routes/session/message-parts.tsx:198-275（InlineToolRow/多 part 段落装配）
  - rust crates/oc-tui/src/components/message/rows.rs:120-135（subagent/多 part 行装配，分段段落）
- **ref 命令面板分组序竞态（groupBy 先到先得，跨运行可翻转）** — 命中 0 case
  - command-palette 场景的 ch 字段差（面板分组顺序差）：ref 的分组序由 `groupBy(category).entries()` 的首现顺序决定（dialog-select.tsx:214-226），而命令选项来自多个异步注册源（app 内建/会话命令//api/command/skills）——注册完成序跨运行可翻转（A2 复跑取证：5 次单跑 1 次 Prompt↔Session 翻转；rust 侧确定性注册序恒 Session 组在前）。ref 树只读不可修，登记为通道 8 已知 ref 侧竞态，非移植语义差
  - ref packages/tui/src/ui/dialog-select.tsx:214-226（groupBy/entries 首现序）+ 多源异步注册（routes/session/index.tsx commands 装配）
  - rust crates/oc-tui/src/components/command_palette.rs（确定性分组装配，无竞态）
- **G-424 rust toast 动作 affordance 恒 'x'（ref '› {label}' + 宽面板）** — 命中 3 case（`anim-toast-window`, `anim-toast-window@120x30`, `anim-toast-window@320x80`）
  - anim-toast-* 场景：带 action 的 toast（MCP 失败 → 'Open MCP servers'）——ref 标题行右对齐渲染动作 affordance '› {label}' 且面板宽度计入该 run（title+marginLeft2+affordance）；rust 恒渲染dismiss 'x'、面板按 title+3 计宽（窄 17 列）。A2 静息/帧序列维度新发现的渲染语义差，登记（G-424）不拦。修复面：toast_geometry/toast_tree_units/right_rail_cell/app 指针 hit-rect 四处联动，且钉住 tree-scenario 冻结 oracle（tools/parity/tree-scenarios）与pty_lane_km 活体用例——超出 A2 harness 范围，留专用 lane 落地
  - ref packages/tui/src/ui/toast.tsx:75-103（affordance() = action ? `› ${label}` : "x"，flexShrink=0 右对齐进面板宽度）
  - rust crates/oc-tui/src/components/toast_solid.rs:75-89（恒 "  x" 两 pad + 'x'）、toast.rs:180-186（title+3 计宽）
- **A2 ref 动画门控夹具伪影（运行中工具行 glyph：rust ⠋ 时钟驱动 vs ref ⋯ 静态）** — 命中 3 case（`anim-tool-progress`, `anim-tool-progress@120x30`, `anim-tool-progress@320x80`）
  - anim-* 场景：rust 帧是 (store, clock) 纯函数（A1）——虚拟时钟逐拍推进使spinner 轮转/caret blink（500ms 周期）/knight 渐变在中间拍真实变化；ref 侧夹具钉 animations:false（动画门控关闭，静态形状）且渲染器时钟注入在整 app 下原生不稳（boot 死锁/捕获段错误，探针记录于 a2-report）——这是夹具伪影而非移植语义差：生产 ref 默认 animations:true 渲染同类动画。登记不拦；事件驱动的逐拍内容变化仍被本维度精确断言
  - ref packages/tui/test/fixture/app.ts:32（config animations:false 钉定）+ component/spinner.tsx:27-31（animations 门控回退静态 ⋯）
  - rust crates/oc-tui/tests/tui_fullscreen_diff_driver.rs（ANIM_TICK_MS=100 逐拍 advance_clock）；oc-tui 渲染无动画门控，时钟驱动（A1 接线）
- **B2 version-skew 簇（退版本加载伪差：插件 2.2.x API 面在 ref 2.0.24 降级/不可 boot）** — 命中 0 case
  - plugin-* 场景且 ref 侧网格呈现失败态（'Plugin failed: <path>' 公告）：在 TUI_DIFF_BUN≥1.4.2 门控下（首轮整屏伪差根因= bun 1.3.10 ResolveMessage 非 Error 子类、host.ts:17-43 吞错守卫重抛——控制者实验实锤，已门控隔离）仍失败的载荷=其 2.2.x 宿主 API 假设在 2.0.24 面上不可满足——退版本加载伪差而非移植 bug，rust 侧 boot 出的渲染整体登记不拦；判定文须引插件调用点与 2.0.24 API 面缺失符号（首轮实证附加形状：omo-slim tui 入口加载后 ref 的 '/' 斜杠面板路径被插件降级破坏）。当前语料 0 命中（双插件在门控下均可 boot）——簇保留接未来真不可 boot 载荷
  - ref packages/plugin/src/host.ts:17-43（吞错守卫；bun≥1.4.2 依赖）+ packages/tui/src/plugin/context.tsx:478-484（失败态公告）
  - rust crates/oc-plugin-host/src/resolve.rs:249-289（包感知 resolve_entry）+ supervisor.rs:2359（load.tui 伴生装载）——rust 侧正常 boot
- **B2 插件注册投影（rust 统一进程 server-entry 装载 vs ref TUI 仅 tui 入口面）** — 命中 3 case（`plugin-panel-open`, `plugin-panel-open@120x30`, `plugin-panel-open@320x80`）
  - plugin-panel-open 场景（palette 搜索 'reflect'）：ref 2.0.24 TUI 夹具只装载 tui 入口（config.plugins 本地目录约定），palette 无 omo-slim 命令（reflect/deepwork 注册在包的 server 入口——生产 ref 由 SERVER 进程装载、经命令 API 到达 TUI，夹具无 server 面）→ 'No results found'；rust 移植为统一进程，TUI 的 PluginHost 同时 boot server+tui 入口，palette 携带 'Plugin · oh-my-opencode-slim' 组命令行。这是加载模型/夹具边界差而非渲染 bug；320×80 档同一网格内 ref 真 Solid 侧栏 section（'▼ OMO-Slim v3.0.3' + Preset/agent 行）vs rust 有界 shim 侧栏为 G-28 形状面共现
  - ref packages/tui/src/plugin/context.tsx:296-307（config.plugins 本地目录条目）+ host.ts:17-43（tui 入口探测；server 入口不经 TUI 装载）
  - rust crates/oc-plugin-host/src/supervisor.rs:2330-2390（entry 装载后伴生 load.tui——统一进程同时装载 server+tui）+ oc-tui/src/plugin_boot.rs project_registrations（palette 行投影）
- **G-425 插件失败态管理屏不列失败入口（/plugins 对话框源表缺失）** — 命中 3 case（`plugin-fallback`, `plugin-fallback@120x30`, `plugin-fallback@320x80`）
  - plugin-fallback 场景（故意坏载荷双侧落失败态）：ref 的失败面三件套=错误 toast（'Plugin failed: <target>' + 'Run /plugins to view details.' + '› Open plugins' affordance）+ footer chip（'⊙ 1 plugin failed /plugins'）+ /plugins 管理屏**列出失败入口行**（'x <path> failed, local' + 'enter view error' 提示）。rust 已对齐 toast+chip（B2-fix surface_boot_failures：解析失败与 guest 装载失败双源）；**管理屏的失败行列表未移植**（command_palette.rs 的 DialogKind::Plugins 仅从命令/首页动作注册投影派生 id 集，装载失败入口不入表 → 'No items available'）——立项 G-425 登记不拦（含 toast affordance 格差=G-424 族共现形状，其修复 lane 连同 oracle 重冻结一并落地）
  - ref packages/tui/src/plugin/context.tsx:460-484（失败态列表+toast）+ feature-plugins/system/plugins.tsx:54-76（源表 TUI/server 双段+状态行）
  - rust crates/oc-tui/src/plugin_boot.rs surface_boot_failures（B2-fix：chip+toast 已对齐）+ components/command_palette.rs:1537-1560（DialogKind::Plugins 有界 id 集投影——失败入口缺失=修 G-425 的面）
- **G-28 插件 TSX 有界（bounded shim）** — 命中 0 case
  - plugin-* 场景的形状类分歧：ref 侧插件以 TSX 活树渲染进 Slot，rust 侧以有界 shim（固定 UI 请求形状）近似——已立项的有界偏差
  - ref packages/tui/src/component/panel-host.tsx:35-55（Slot path="session.panel"，TSX 活树 + reactive props）
  - rust crates/oc-tui/src/plugin_ui.rs:15-108（apply_plugin_ui 有界 shim，UiRequest 中性事件形状）
- **G-332 markdown 有界行形状** — 命中 0 case
  - session-markdown-* 场景且 ch 字段差（行形状分歧）：ref 经 markdown 渲染器 memo 重建整块，rust 经分派表逐行缓存渲染——折行/代码块边界行的有界形状差
  - ref packages/tui/src/plugin/markdown.ts:7-18（createMarkdownRenderer memo，renderNode 变更整块重建）
  - rust crates/oc-tui/src/plugin_markdown.rs:82-147（分派表 install_table/cached_rows/spawn_render 逐行缓存）
- **G-239/G-331 panel 有界（PanelFingerprint）** — 命中 0 case
  - panel-* 场景：PanelInput 的 focus()/close()/toggleFullscreen() 闭包无法过线，rust 以 PanelFingerprint 快照近似 reactive props——已立项的有界偏差
  - ref packages/tui/src/component/panel-host.tsx:36-55（Slot input 的 reactive width/presentation/focused + 闭包 props）
  - rust crates/oc-tui/src/plugin_boot.rs:100（PanelFingerprint）、:425-440（session.panel props 有界化）、:767-780（fingerprint 快照）

未命中任何簇的分歧保持未裁决进入 T9；CAUSES 不为绿灯放宽 matcher。

## frames 维度（A2 帧序列对拍）

anim 场景（`anim.ticks` 声明，SCHEMA A2 扩展）在静息排空后逐拍对拍（ANIM_TICK_MS=100ms，k=0..K 共 K+1 帧）：逐拍 EXACT + 首异拍定位。rust 侧 store.advance_clock 纯函数帧（A1）；ref 侧事件驱动逐拍（tick_events 注入 + 双 idle/render 泵，无虚拟时钟——ManualClock 整 app 注入原生不稳，SCHEMA comparability 声明）。G-128 midframe / G-49·G-368 节奏簇仅在「首拍与末拍一致、仅中间拍差」时豁免（登记不拦）；末拍也差的不是 midframe 族——未命中即未裁决。

| anim 总量 | EXACT | CAUSES 命中 | 未裁决 |
|---|---|---|---|
| 15 | 6 | 9 | 0 |

| case | verdict | 非exact拍 | per-tick | first 异拍 |
|---|---|---|---|---|
| anim-spinner-running | exact | 0/9 | `.........` (K=8) | — |
| anim-spinner-running@120x30 | exact | 0/9 | `.........` (K=8) | — |
| anim-spinner-running@320x80 | exact | 0/9 | `.........` (K=8) | — |
| anim-stream-delta | exact | 0/11 | `...........` (K=10) | — |
| anim-stream-delta@120x30 | exact | 0/11 | `...........` (K=10) | — |
| anim-stream-delta@320x80 | exact | 0/11 | `...........` (K=10) | — |
| anim-stream-multi-part | diverge | 10/11 | `.XXXXXXXXXX` (K=10) | tick=1 r6c5 ch ref=' ' rust='A' |
| anim-stream-multi-part@120x30 | diverge | 10/11 | `.XXXXXXXXXX` (K=10) | tick=1 r6c5 ch ref=' ' rust='A' |
| anim-stream-multi-part@320x80 | diverge | 10/11 | `.XXXXXXXXXX` (K=10) | tick=1 r6c5 ch ref=' ' rust='A' |
| anim-toast-window | diverge | 13/13 | `XXXXXXXXXXXXX` (K=12) | tick=0 r1c27 ch ref='┃' rust=' ' |
| anim-toast-window@120x30 | diverge | 13/13 | `XXXXXXXXXXXXX` (K=12) | tick=0 r1c67 ch ref='┃' rust=' ' |
| anim-toast-window@320x80 | diverge | 13/13 | `XXXXXXXXXXXXX` (K=12) | tick=0 r1c267 ch ref='┃' rust=' ' |
| anim-tool-progress | diverge | 11/11 | `XXXXXXXXXXX` (K=10) | tick=0 r7c5 ch ref='⋯' rust='⠋' |
| anim-tool-progress@120x30 | diverge | 11/11 | `XXXXXXXXXXX` (K=10) | tick=0 r7c5 ch ref='⋯' rust='⠋' |
| anim-tool-progress@320x80 | diverge | 11/11 | `XXXXXXXXXXX` (K=10) | tick=0 r7c5 ch ref='⋯' rust='⠋' |

- `anim-stream-multi-part`  — CAUSES: G-423 多 text part 内联拼接渲染差（anim-stream-multi-part）
- `anim-stream-multi-part@120x30`  — CAUSES: G-423 多 text part 内联拼接渲染差（anim-stream-multi-part）
- `anim-stream-multi-part@320x80`  — CAUSES: G-423 多 text part 内联拼接渲染差（anim-stream-multi-part）
- `anim-toast-window`  — CAUSES: G-424 rust toast 动作 affordance 恒 'x'（ref '› {label}' + 宽面板）
- `anim-toast-window@120x30`  — CAUSES: G-424 rust toast 动作 affordance 恒 'x'（ref '› {label}' + 宽面板）
- `anim-toast-window@320x80`  — CAUSES: G-424 rust toast 动作 affordance 恒 'x'（ref '› {label}' + 宽面板）
- `anim-tool-progress`  — CAUSES: A2 ref 动画门控夹具伪影（运行中工具行 glyph：rust ⠋ 时钟驱动 vs ref ⋯ 静态）
- `anim-tool-progress@120x30`  — CAUSES: A2 ref 动画门控夹具伪影（运行中工具行 glyph：rust ⠋ 时钟驱动 vs ref ⋯ 静态）
- `anim-tool-progress@320x80`  — CAUSES: A2 ref 动画门控夹具伪影（运行中工具行 glyph：rust ⠋ 时钟驱动 vs ref ⋯ 静态）

## recorded-local（真实语料，聚合计数）

本地真实会话语料（`tools/diff/corpus/tui/recorded-local/`，git-ignored；
隐私铁律：报告只列聚合计数，不引用录制内容原文，分歧明细表亦不展开）。

| 维度 | 总量 | EXACT | STRUCTURAL | DIVERGE | 未裁决 |
|---|---|---|---|---|---|
| resting | 9 | 3 | 0 | 6 | 6 |

## 范围外声明

- /opt/opentui-rust 127 场景 = 框架层（OpenTUI 渲染器）边界，本通道只测 oc-tui 应用层，
  不在其上做差分裁决
- 动画中间帧 G-128/G-49/G-368：A2 起有了 frames 维度的可断言面（anim 场景逐拍对拍，上节）；纯静息场景仍只比静止帧，frames 簇豁免收窄为「首末拍一致、仅中间拍差」的 midframe 形状
- ref 侧不可虚拟时钟化的动画面：toast 解散（真实全局 setTimeout，ui/toast.tsx:142）与 caret blink（原生 cursor 样式，captureSpans 不可见）——帧序列不覆盖（SCHEMA comparability 声明）
- 活体交互序列（键位/焦点/resize 时序）= v2 扩展，静止帧差分不覆盖
- 双侧 request_census 不对称 = 信息性（boot 请求模式已知不同），不参与判门

