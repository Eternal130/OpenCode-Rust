# 搁置项条件核查 X-B（TUI/渲染类）— 2026-11-10，HEAD=967bedba

**G-128 title-shimmer 静止态 → CONDITION_MET（实质已落地，仅剩文档陈旧）**
- store 已有 rename-pending：`store.rs:2067-2068`（title_pending Signal，注释 "title_shimmer envelope's input"）、`:4422-4428`（set_title_pending）；写入/清除已接（app.rs:9144-9152 POST rename、store.rs:12816 回落）。
- 动效本体已渲染：`components/sidebar.rs:435-469`（sustained 40% dim smootherstep 240ms + 1200ms 扫光带 shimmer_fg per-char runs）进 SidebarPaint，sidebar_solid 消费（sidebar_solid.rs:8-13 "both painters consume the ONE SidebarPaint"）。静止态=ref at-rest bold text.base，自洽。
- 问题：`sidebar_solid.rs:15-28` 模块头仍 declare "no rename-pending input exists"——**stale 注释**，与现状矛盾。
- 建议：改头注指向 store.title_pending + sidebar.rs:435；GAPS G-128 勘注标闭。纯文档 <30min。（ref arrival-wipe 相相若计入另 +40 行。）

**G-272⑩ mini 样式化 surface → CONDITION_MET（基建已备，接入点存在）**
- mini footer 已走 opentui 真引擎（ADR-0019 D2.3）；opentui-core@56a9eda `ScrollbackSurface`（renderer/scrollback.rs:123+）+ renderables 全家（text/code/markdown/markdown_parser/text_table，settle=render）能力齐全。
- 缺口：`ExternalOutputCommit.lines: Vec<String>`（scrollback.rs:85-92）只携纯文本行，样式过不了闸；row_widths 对 ANSI 字节误计宽。
- 修法：commit 管道加 styled 变体（styled cells/ANSI 串+宽度旁路），mini Commit 从 mono 直写改为按 kind 挂 renderable，经 commit FIFO 落转录区；pty 金样重钉。规模：载荷扩展+宽度数学 ~150 行 + renderable 消费层 ~500-800 行，含差分钉定 **3-5 天**。

**G-239 panel/PanelHost → PARTIAL（基建部分具备，无消费方）**
- 仍无 panel API（oc-plugin-host/ui.rs 无 panel 符号；plugin_slots.rs:46 session.panel 明列 deliberate unmounted，tests/plugin_ui_slot.rs:146-150 断言）。
- 可用基建已就位：slot 解析/投影 + mini dock 几何 + 共享树 mount 通道（plugin_boot.rs:376-381）。
- 最小路径若立项：session.panel 入 MOUNTED_SLOT_PATHS、dock 区 renderable 承载、PanelInput 有界命令对（~300-450 行）。**无 shipping plugin 认领 → 维持搁置合理**。

**G-211 split-footer → OBSOLETE（登记条目本身过期）**
- GAPS:1293 收官注（2026-11-05）已宣布 "G-211 已全量移植并消费（ADR-0019 终稿）"；makeRoom/ExternalOutputCommit/ScrollbackSurface/DECSTBM 全在（opentui-core renderer/scrollback.rs、mini/lifecycle.rs:1710-1845、pty_daily.rs:2507/2580）。
- 建议：GAPS:1010-1012 旧条目标 OBSOLETE 指向 1293 注（登记账内部自相矛盾清理），零代码。

**G-200② terminalHandoff → 维持登记 + 补一行勘注**
- ① CLI preflight 仍 stderr（misc.rs:31-45 注释 "the Rust CLI has no opentui runtime"）；Mismatch::Replace 仍终端获取前同步解决（misc.rs:56）。
- ② oc-tui app.rs:7953/8014 自建渲染器自管生命周期，无 acquire/release/注入接口。
- ③ 新事实：misc.rs:80 CLI 与 TUI **同进程**（`oc_tui::app::run(client, boot)`）——移交架构上变得可行，但替换窗口无渲染器存在、ref handoff 解决的问题在此不出现。
- 建议：GAPS:1318 补勘注一行，维持文档化决策。
