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
| 40 | 0 | 0 | 0 | 40 |

（verdict 分布：exact 0 / structural 0 / diverge 40；
gate 见编排器输出，未裁决>0 ⇒ exit 2）

## 分歧明细

| case | verdict | diff cells | first_diff | ref size | rust size | ref gaps | rust gaps | ref paths | rust paths |
|---|---|---|---|---|---|---|---|---|---|
| command-palette | diverge | 353 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| command-palette@120x30 | diverge | 527 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| dialog-open-recent | diverge | 462 | r0c7 attrs ref=[] rust=['bold'] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| dialog-open-recent@120x30 | diverge | 688 | r0c7 attrs ref=[] rust=['bold'] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| dialog-status | diverge | 1860 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| dialog-status@120x30 | diverge | 3600 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| fetch-503-model | diverge | 42 | r15c6 ch ref=' ' rust='B' | 80x24 | 80x24 | 0 | 0 | 20 | 11 |
| fetch-503-model@120x30 | diverge | 42 | r18c26 ch ref=' ' rust='B' | 120x30 | 120x30 | 0 | 0 | 20 | 11 |
| home-empty | diverge | 42 | r15c6 ch ref=' ' rust='B' | 80x24 | 80x24 | 0 | 0 | 20 | 11 |
| home-empty@120x30 | diverge | 42 | r18c26 ch ref=' ' rust='B' | 120x30 | 120x30 | 0 | 0 | 20 | 11 |
| home-multi-session | diverge | 269 | r17c2 ch ref='┃' rust=' ' | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| home-multi-session@120x30 | diverge | 389 | r23c2 ch ref='┃' rust=' ' | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| models-dialog | diverge | 1920 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| models-dialog@120x30 | diverge | 3600 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| session-empty | diverge | 269 | r17c2 ch ref='┃' rust=' ' | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-empty@120x30 | diverge | 389 | r23c2 ch ref='┃' rust=' ' | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| session-markdown | diverge | 612 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-markdown@120x30 | diverge | 892 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| session-multi-turn | diverge | 1071 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-multi-turn@120x30 | diverge | 2602 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| session-reasoning | diverge | 764 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-reasoning@120x30 | diverge | 1124 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| session-scroll | diverge | 1036 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-scroll@120x30 | diverge | 1910 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| session-step-failed | diverge | 642 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-step-failed@120x30 | diverge | 922 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| session-stream-idle | diverge | 298 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-stream-idle@120x30 | diverge | 418 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| session-tabs | diverge | 1860 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-tabs@120x30 | diverge | 3600 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| session-text | diverge | 610 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-text@120x30 | diverge | 890 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| session-tool-failed | diverge | 492 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-tool-failed@120x30 | diverge | 692 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| session-tool-success | diverge | 766 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-tool-success@120x30 | diverge | 1126 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| session-wrap | diverge | 617 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-wrap@120x30 | diverge | 897 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| sidebar-toggle | diverge | 1633 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| sidebar-toggle@120x30 | diverge | 3273 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |

### first_diff 明细

- `command-palette` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=353 — 未裁决（T9 候选）
- `command-palette@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=527 — 未裁决（T9 候选）
- `dialog-open-recent` [diverge] row=0 col=7 field=attrs ref=[] rust=['bold'] cells_diff_count=462 — 未裁决（T9 候选）
- `dialog-open-recent@120x30` [diverge] row=0 col=7 field=attrs ref=[] rust=['bold'] cells_diff_count=688 — 未裁决（T9 候选）
- `dialog-status` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=1860 — 未裁决（T9 候选）
- `dialog-status@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=3600 — 未裁决（T9 候选）
- `fetch-503-model` [diverge] row=15 col=6 field=ch ref=' ' rust='B' cells_diff_count=42 — 未裁决（T9 候选）
- `fetch-503-model@120x30` [diverge] row=18 col=26 field=ch ref=' ' rust='B' cells_diff_count=42 — 未裁决（T9 候选）
- `home-empty` [diverge] row=15 col=6 field=ch ref=' ' rust='B' cells_diff_count=42 — 未裁决（T9 候选）
- `home-empty@120x30` [diverge] row=18 col=26 field=ch ref=' ' rust='B' cells_diff_count=42 — 未裁决（T9 候选）
- `home-multi-session` [diverge] row=17 col=2 field=ch ref='┃' rust=' ' cells_diff_count=269 — 未裁决（T9 候选）
- `home-multi-session@120x30` [diverge] row=23 col=2 field=ch ref='┃' rust=' ' cells_diff_count=389 — 未裁决（T9 候选）
- `models-dialog` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=1920 — 未裁决（T9 候选）
- `models-dialog@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=3600 — 未裁决（T9 候选）
- `session-empty` [diverge] row=17 col=2 field=ch ref='┃' rust=' ' cells_diff_count=269 — 未裁决（T9 候选）
- `session-empty@120x30` [diverge] row=23 col=2 field=ch ref='┃' rust=' ' cells_diff_count=389 — 未裁决（T9 候选）
- `session-markdown` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=612 — 未裁决（T9 候选）
- `session-markdown@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=892 — 未裁决（T9 候选）
- `session-multi-turn` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=1071 — 未裁决（T9 候选）
- `session-multi-turn@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=2602 — 未裁决（T9 候选）
- `session-reasoning` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=764 — 未裁决（T9 候选）
- `session-reasoning@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=1124 — 未裁决（T9 候选）
- `session-scroll` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=1036 — 未裁决（T9 候选）
- `session-scroll@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=1910 — 未裁决（T9 候选）
- `session-step-failed` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=642 — 未裁决（T9 候选）
- `session-step-failed@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=922 — 未裁决（T9 候选）
- `session-stream-idle` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=298 — 未裁决（T9 候选）
- `session-stream-idle@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=418 — 未裁决（T9 候选）
- `session-tabs` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=1860 — 未裁决（T9 候选）
- `session-tabs@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=3600 — 未裁决（T9 候选）
- `session-text` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=610 — 未裁决（T9 候选）
- `session-text@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=890 — 未裁决（T9 候选）
- `session-tool-failed` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=492 — 未裁决（T9 候选）
- `session-tool-failed@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=692 — 未裁决（T9 候选）
- `session-tool-success` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=766 — 未裁决（T9 候选）
- `session-tool-success@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=1126 — 未裁决（T9 候选）
- `session-wrap` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=617 — 未裁决（T9 候选）
- `session-wrap@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=897 — 未裁决（T9 候选）
- `sidebar-toggle` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=1633 — 未裁决（T9 候选）
- `sidebar-toggle@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=3273 — 未裁决（T9 候选）

## CAUSES 定性

- **G-368 量化色（oklab 最近邻量化差）** — 命中 0 case
  - 仅 RGB 差且每通道 ≤RGB_TOLERANCE(8)、char/attrs 全等：双侧滚动区主题各经一次 oklab 感知最近邻量化（同一算法、不同浮点环境），量化格点选择可相邻一格——结构性色彩差，非语义差
  - ref packages/tui/src/mini/theme.ts:81-105（oklab/nearestIndexed），经 mini/theme.ts:117-125 V1 序列化恢复后量化
  - rust crates/oc-tui/src/mini/theme.rs:114-192（oklab 量化 nearest_indexed/scrollback_color，1:1 移植）
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

## 范围外声明

- /opt/opentui-rust 127 场景 = 框架层（OpenTUI 渲染器）边界，本通道只测 oc-tui 应用层，
  不在其上做差分裁决
- 动画中间帧 G-128/G-49/G-368 豁免：本通道只比对静止帧（resting frame）
- 活体交互序列（键位/焦点/resize 时序）= v2 扩展，静止帧差分不覆盖
- 双侧 request_census 不对称 = 信息性（boot 请求模式已知不同），不参与判门

