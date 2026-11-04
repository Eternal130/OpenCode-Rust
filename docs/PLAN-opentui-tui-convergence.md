# PLAN — opencode-rust TUI 全量收敛（opentui-rust 单源化）

> deepwork 任务：`.slim/deepwork/tui-full-convergence/`（Gate D0 oracle 已批准，attempt 1/3，2026-11-03）。
> 目标：TUI 体验与 ref（v2.0.24）完全一致（忽略性能）；ref 中由 opentui 承担的功能 → opentui-rust 承担；oc-tui 超集全部移除；opentui-rust 与 opentui 契约+API 一致。
> 体验 oracle：`tools/parity/goldens/tui-frames/`（**ref 导出语料**）+ PTY harness；架构 oracle：opentui-rust 差分套件（119 场景 IDENTICAL）+ PORT-MATRIX。

## 相位（Gate D0 批准版）

### Phase D1 — opentui-rust 渲染器运行时与缺口移植（纯增量，不动 oc-tui；每 lane=差分门+recount+矩阵诚实化）
- **D1a** CliRenderer 运行时生命周期 + 完整 CliRendererConfig：requestRender/destroy/isDestroyed/setBackgroundColor/setTerminalTitle/waitForThemeMode/setMaxListeners/useMouse 切换、stdin/clearOnShutdown/exitOnCtrlC/gatherStats/consoleMode("disabled")/consoleOptions；渲染循环建于 Phase A terminal session（feature-gated，默认 headless 不变）；三契约 oracle=①settle 语义（N 轮 requestRender→恰 M 帧确定性 flush，无饥饿/过度合并）②脏跟踪不变量（clean 树+requestRender=零字节；无 requestRender 的变更=零字节）③生命周期状态机（destroy 后调用=no-op 非 panic）；矩阵 CliRenderer(L222)/CliRenderEvents(L221) done→partial 修正
- **D1b-core** split-footer 运行时基底：writeToScrollback core write + EXTERNAL_OUTPUT/CliRendererExternalOutputEvent + makeRoom/ExternalOutputCommit
- **D1b-surface** ScrollbackSurface/Snapshot/RenderContext + screenMode split-footer/main-screen + footerHeight + externalOutputMode capture-stdout/passthrough（**正式闭 G-211**）
- **D1c** 小 API 包：TextBufferView(mini footer.view)、OnHighlightCallback(patch-diff 实时高亮)、HostClipboard 后端(OSC52/pbcopy)、EditorTraits、TerminalColors、ensureRuntimePluginSupport（**契约级**：configure() 的 solid 运行时初始化语义，非 bun loader 机制——mechanism≠upstream 登记）
- **D1d** oc-syntax 降格为 grammar vendor：fence/diff/tool_rows/theme 高亮切 opentui TreeSitterClient（add_default_parsers 注册 oc-syntax 目录）；删 style.rs 边界转换；顺带闭 G-13 injection 项（与 D1a/b/c、D2 零依赖故提前）

### Phase D2 — oc-tui 渲染管线收敛（ADR-0019 推翻 a1；**渐进双轨**）
- **D2.1** 渲染循环所有权 → CliRenderer（GridEmitter 仍发射，发射缝最后动）
- **D2.2** 逐表面迁 renderable 树（chrome→composer→transcript→footer→dialogs），每 lane goldens 门
- **D2.3** GridEmitter 退役 + mini 薄缝→真 split-footer + G-211 关账；**显式重裁 ADR-0018 各登记偏差**（mini no-scrolling-regions/resize 不重排等——保留与否是裁决不是事故）
- **硬门**：goldens 零 diff 且 **PARITY_UPDATE 在 D2 全程不得触发**（需重钉=stop-and-escalate）；`mini_pty.rs` 4 测试 + mini-frames 金样全绿

### Phase D3 — 表面与功能族收敛（消费方拉动）
which-key 面板全量（ref 本地 feature plugin 侧，layout/scroll/group/pending + 11 键位接通）→ terminal_pane→EmbeddedTerminalRenderable（**折叠 oc-tui 468 行 VT 引擎**，mechanism≠upstream/PTY 网格平价钉定，登记 ADR-0019）→ image 族迁入 opentui-rust（kitty/sixel/blocks protocol="auto"，oc-tui image/* 为种子）+ oc-tui 消费 ImageRenderable → diff_viewer solid 化（2308 行最大）→ tool_rows/marquee/masked_text/update/tab_pulse → 超集终扫（现有证据≈零）

### Phase D4 — 收官
ADR-0019 + G-211/G-13 勘注、矩阵 recount、全量门（双 profile+goldens+PTY）、磁盘清理、tombstone。

## 铁则
见 `.slim/deepwork/tui-full-convergence/progress.md` frozen 节（GAPS-AND-PITFALLS 全制度：dev-gate 单入口、worktree 隔离、lane 禁碰 main checkout、tee 逐字内联、nextest 不过滤、合并门、PTY-only TUI、45 分钟分块、子代理无后台、English commits、recount 钩子、已知 flake 白名单等）。
