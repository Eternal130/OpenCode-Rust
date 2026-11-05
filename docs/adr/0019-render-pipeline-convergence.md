# 0019 — oc-tui 渲染管线收敛至 opentui-rust 单源（推翻 ADR-0018 的 a1/薄缝裁决）

- 状态：accepted（2026-11-05 定稿；D0–D4 全相落地，见文末证据）
- 取代：ADR-0018 决策 1（GridEmitter a1 路线）与决策 5（mini 薄缝）——两裁决标的物均已从代码删除
- 范围：crates/oc-tui（+ oc-cli mini 前端）与 opentui-rust 渲染面的接线关系

## 决策（终态）

1. **渲染循环与字节出口归 opentui**：主 TUI 泵经活 CliRenderer 调度（request_render/settle 门控），帧发射走 `render/opentui_emit.rs` 落 opentui 发射器，?2026 信封经渲染器泵路径；**GridEmitter 已删除**（B3b 以来的旧差分门由 D4.2 活差分证明字节一致后退役，证据冻结为 `opentui_emit_diff_tests` 双摘要：语料 0x3bdbd28c1166a117、词表 0xf748ba520bf0d923 + 逐帧 cursor 政策断言 + cluster vt-roundtrip 像素钉）。
2. **单一共享树**：全部活表面（chrome/transcript/composer/footer/dialogs/overlays/sidebar/diff/terminal-pane/tabs-strip/home/picker/queued-dock）以绝对定位 wrapper 子树上树（子序=z）；入帧唯一缝=一次全缓冲合成（洞跳过/缝上卸载/stamps 家族：Caret/Swap/Transparent/DimBg/FgDefault）；`paste_frame` 已删除。
3. **mini 全量真引擎**：`CliRenderer{SplitFooter, footer_height, CaptureStdout}`；转录走 `write_to_scrollback`→`flush_pending_split_commits`（DECSTBM 转录区）；**footer 内容挂渲染器 root 树**（D4.1c-pre 移植 render-geometry/带高布局/renderOffset 发射/footer 泵路径/solid render_into/set_footer_height 六缺口后，D4.1c 落消费）——「永不 pump」裁决退役，帧字节经 ?2026 泵出口。
4. **opentui-rust 契约面补齐**（本 ADR 周期）：CliRenderer 运行时生命周期+完整配置；split-footer 运行时+surface 家族（G-211 闭）；TextBufferView/OnHighlightCallback/HostClipboard/EditorTraits/TerminalColors/ensureRuntimePluginSupport；native image 渲染（drawImage/kitty/sixel/blocks/protocol=auto/cell-px/遮挡——C3 三缺口闭）；EmbeddedTerminal OSC 4/104；footer 域渲染侧。上游 file:line 引用见 PORT-MATRIX 与各 lane 台账。
5. **K 类持牌例外**：`error_component_frame`（update.rs）保留 Frame 形状——渲染崩溃兜底不得依赖其崩溃的树/缝；唯一持牌 Frame 生产者。
6. **Frame 内部货币登记**：`render::Frame` 作为装配/孪生货币存续（~45 文件：E/S/R/H 孪生与装配产物；非渲染后端——ratatui 自 Phase B 已出库，Frame 为 oc-tui 自有类型）。**不再是字节出口**（出口=opentui 发射器）。

## 偏差账（终版）

- **已关**：split-footer/ExternalOutputCommit 延后（G-211）；mini resize 不重排（M3）；行号槽结构性分歧（D3-5）；GridEmitter a1（本 ADR 主旨）；「永不 pump」（D4.1c）。
- **保留（ADR-0018 承继）**：resize ≤50ms 轮询语义；resize 检测寄生 tick（M5 上界=run-loop tick）；panic hook 线程门控；OSC 8 无槽回收+link-id 门；kitty push 字节留 oc-tui；孤立 ESC flush；鼠标 feed 前拦截。
- **保留（D2.2 登记）**：dialog scrim=post-seam 带行走；console/reconnecting 与 dialog 带行走共见 z 微偏离（方向登记，共见瞬态）；toast 右轨宽字簇裁剪单格 bg 微偏差；pill/migration 座 8/9。
- **保留（D3 登记项）**：terminal_pane 折叠引擎 mechanism≠upstream（vte vs ghostty-vt；18 et-* 差分+PTY 网格平价钉定）；image 六项（PNG 压缩解码 C3-deferred、kitty zlib/file 传输未移植、sixel 透明度近似、无增量脏缓存、refusesForcedSixel 未建模、resize 重查询状态机未移植）；diff wrapMode None 保真；嵌入终端字素簇/tab stop/kitty 词表收窄；OSC 4 单向。
- **C 类不移植（G-17 族，G-212 登记）**：OPENCODE_STORY/ROUTE/DRIVE/NODE_ASSETS_DIR/TMUX-STY 探测（测试/storybook 管道）。

## 证据指针

- 全 lane 台账与门日志：`.slim/deepwork/tui-full-convergence/topic-*.md` + `.test-logs/oc-tui-*` / `opentui-core-*`
- 差分冻结：`crates/oc-tui/src/opentui_emit_diff_tests.rs`
- 金样全程零 diff、PARITY_UPDATE 全程未触发（tui-frames=ref 导出语料）
