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
| 40 | 28 | 0 | 0 | 12 |

（verdict 分布：exact 28 / structural 0 / diverge 12；
gate 见编排器输出，未裁决>0 ⇒ exit 2）

## 分歧明细

| case | verdict | diff cells | first_diff | ref size | rust size | ref gaps | rust gaps | ref paths | rust paths |
|---|---|---|---|---|---|---|---|---|---|
| command-palette | diverge | 112 | r0c10 attrs ref=[] rust=['bold'] | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| command-palette@120x30 | diverge | 166 | r0c10 attrs ref=[] rust=['bold'] | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| dialog-open-recent | diverge | 275 | r0c7 attrs ref=[] rust=['bold'] | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| dialog-open-recent@120x30 | diverge | 305 | r0c7 attrs ref=[] rust=['bold'] | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| dialog-status | diverge | 218 | r0c9 attrs ref=[] rust=['bold'] | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| dialog-status@120x30 | diverge | 218 | r0c9 attrs ref=[] rust=['bold'] | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| fetch-503-model | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 20 | 12 |
| fetch-503-model@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 20 | 12 |
| home-empty | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 20 | 12 |
| home-empty@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 20 | 12 |
| home-multi-session | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| home-multi-session@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| models-dialog | diverge | 8 | r0c9 attrs ref=[] rust=['bold'] | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| models-dialog@120x30 | diverge | 8 | r0c9 attrs ref=[] rust=['bold'] | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| session-empty | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| session-empty@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| session-markdown | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| session-markdown@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| session-multi-turn | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| session-multi-turn@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| session-reasoning | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| session-reasoning@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| session-scroll | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| session-scroll@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| session-step-failed | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| session-step-failed@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| session-stream-idle | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| session-stream-idle@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| session-tabs | diverge | 326 | r0c6 attrs ref=[] rust=['bold'] | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| session-tabs@120x30 | diverge | 422 | r0c6 attrs ref=[] rust=['bold'] | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| session-text | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| session-text@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| session-tool-failed | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| session-tool-failed@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| session-tool-success | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| session-tool-success@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| session-wrap | exact | 0 | — | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| session-wrap@120x30 | exact | 0 | — | 120x30 | 120x30 | 0 | 0 | 27 | 15 |
| sidebar-toggle | diverge | 141 | r2c5 fg ref=[173, 173, 173] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 15 |
| sidebar-toggle@120x30 | diverge | 195 | r2c5 fg ref=[173, 173, 173] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 15 |

### first_diff 明细

- `command-palette` [diverge] row=0 col=10 field=attrs ref=[] rust=['bold'] cells_diff_count=112 — 未裁决（T9 候选）
- `command-palette@120x30` [diverge] row=0 col=10 field=attrs ref=[] rust=['bold'] cells_diff_count=166 — 未裁决（T9 候选）
- `dialog-open-recent` [diverge] row=0 col=7 field=attrs ref=[] rust=['bold'] cells_diff_count=275 — 未裁决（T9 候选）
- `dialog-open-recent@120x30` [diverge] row=0 col=7 field=attrs ref=[] rust=['bold'] cells_diff_count=305 — 未裁决（T9 候选）
- `dialog-status` [diverge] row=0 col=9 field=attrs ref=[] rust=['bold'] cells_diff_count=218 — 未裁决（T9 候选）
- `dialog-status@120x30` [diverge] row=0 col=9 field=attrs ref=[] rust=['bold'] cells_diff_count=218 — 未裁决（T9 候选）
- `models-dialog` [diverge] row=0 col=9 field=attrs ref=[] rust=['bold'] cells_diff_count=8 — 未裁决（T9 候选）
- `models-dialog@120x30` [diverge] row=0 col=9 field=attrs ref=[] rust=['bold'] cells_diff_count=8 — 未裁决（T9 候选）
- `session-tabs` [diverge] row=0 col=6 field=attrs ref=[] rust=['bold'] cells_diff_count=326 — 未裁决（T9 候选）
- `session-tabs@120x30` [diverge] row=0 col=6 field=attrs ref=[] rust=['bold'] cells_diff_count=422 — 未裁决（T9 候选）
- `sidebar-toggle` [diverge] row=2 col=5 field=fg ref=[173, 173, 173] rust=[238, 238, 238] cells_diff_count=141 — 未裁决（T9 候选）
- `sidebar-toggle@120x30` [diverge] row=2 col=5 field=fg ref=[173, 173, 173] rust=[238, 238, 238] cells_diff_count=195 — 未裁决（T9 候选）

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

