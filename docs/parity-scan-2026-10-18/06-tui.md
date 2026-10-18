# TUI 子系统 parity 扫描（parity-scan 2026-10-18 · 06-tui）

- 参考实现：`/opt/opencode-ref`（opencode v2.0.24，`packages/tui/**` 314 文件 ≈61.7k 行 + `packages/theme/tui/**`）
- 目标实现：`/opt/opencode-rust`（`crates/oc-tui/**`、`crates/oc-tui-core/**`、`crates/oc-theme/**` ≈74.9k 行）
- 方法：只读结构对扫（路由枚举 → DialogKind 枚举 → 组件模块 → 键位表 diff → 主题资产/token diff → 协议序列 → 既有审计文档交叉核对）。所有结论以**当前 main（bb364b6）**代码为准；早先 auditA/auditB（beta.8 时代）中已被后续 lane 修复的条目全部重新验证并标注。
- 分类：`MISSING`（无表面）/ `STUB`（占位）/ `PARTIAL`（有但浅）/ `WRONG`（行为不同）/ `EXTRA`（rust 多出）。级别：S0 日常用户可见、S1 高频/条件可见、S2 低频/开发向。

## 0. 总览

| 维度 | 结论 |
|---|---|
| 路由 | home/session 完整（含 tab 生命周期、恢复、fork、prompt 注入）；**plugin 路由缺失**（storybook/stats/plugins 路由不可达） |
| 对话框 | ref ~40 个对话框面：**31 个已实质移植**（含 Integration 5 视图、Settings、Timeline、Fork、Export、Update、Mcp、ToolOutput、DiffHelp 等，此前 auditB 的 12 个 STUB 已基本清零）；**6 类缺失**（Skill/Stash/ImagePreview/Experiments/ErrorDetails(通用)/Alert+Confirm）+ `open.menu` 映射错位 |
| 键位表 | **231/231 条 ref `Definitions` 全覆盖**（含 which-key 12 条、composer 家族），+8 条 rust-only（`toast.activate` 端口增补等）；dispatch 层有约 25 条"注册即吞键"的无表面命令（与 v2 的无表面对齐或因端口缺表面） |
| 主题 | 29 个 v1 资产 + v2/opencode.json **语义全等**（JSON 归一后 0 diff）；token schema 覆盖 success/info/scrollbar/formfield/hovered 等 auditB 时缺失的键；自定义主题发现 ✓ |
| 终端协议 | kitty query/push/pop、DECSCUSR+OSC12、?2027/?2031、SGR-256 降级矩阵 = **termatrix TM-1/2/3/4 已修**；**OSC 标题（TM-5）与 sync-2026（TM-6）仍缺** |
| 插件 UI | `ctx.ui` 桥只有 toast/文本行/footer 提示/dialog（rescan F1/F7 KNOWN S0）；原生 plugin sidebar 分区 seam 存在（SA-F5） |
| mini | 运行时/队列/回滚/splash/transport/demo 齐；**子代理 inspector（footer.subagent + stream-v2.subagent）缺失** |
| 声音/通知/剪贴板 | 5 音频资产 + OSC 9/99/777 + OSC52 读写 ✓；播放走系统播放器（已登记分歧） |

计数：本次登记差异 **34 项**（S0 ×2、S1 ×15、S2 ×17），其中 KNOWN（已有文档登记）19 项、NEW 15 项；另确认“已修复”的早先审计条目 21 项（附录 A）。

---

## 1. 路由 / 屏幕映射表

ref 路由模型 = `context/route.tsx`（home / session / plugin）+ 启动注入（`OPENCODE_ROUTE`/`OPENCODE_STORY`，app.tsx:335-352）。

| ref 屏幕 | ref 位置 | rust 对应 | rust 位置 | 状态 |
|---|---|---|---|---|
| Home | routes/home.tsx | `Route::Home` + home.rs/home_solid.rs | store.rs:45；components/home.rs | PORTED（logo/占位池/banner/MCP cell/版本行） |
| Session | routes/session/index.tsx (3591 行) | `Route::Session` + session_screen.rs/session.rs | store.rs:47；components/session_screen.rs:352 | PORTED（差异见 §3/§14） |
| Plugin 路由（含 storybook/stats/plugins 面板） | app.tsx:337-352,1370-1376；feature-plugins/system/{stats,plugins,storybook} | **无**（Route 仅 Home/Session） | store.rs:44-49 | **MISSING**（NEW，S2——storybook 为 dev；但 `OPENCODE_ROUTE` 任意插件路由是产品能力） |
| SessionFrame（右栏 sidebar/terminal/panel 三态 + 全屏面板） | component/session-frame.tsx；context/panel.tsx；panel-host.tsx | 仅 sidebar 一态 | components/session_screen.rs | PARTIAL（terminal pane 与 fullscreen panel 缺，见 #14/#15） |
| 垂直 tab 栏 + 拖拽调宽 + 持久化 | app.tsx:574-585,1347-1349,1382-1384；ui/pane-resize*.ts* | **无**（strip 只有水平 `paint_tabs_bar`；无 `tabs.layout` config） | components/session_screen.rs:270 | **MISSING**（KNOWN-adjacent：auditB §1 登记、未修，NEW 确认仍缺，S1） |
| StartupLoading（500ms→"Loading plugins…"→3s hold） | app.tsx:1389-1391；startup-loading.tsx | overlays.rs + store 状态机 | components/overlays.rs:131-142 | PORTED（修复确认） |
| Reconnecting（1s/5s 延迟、managed 文案） | app.tsx:1294-1315,1392-1394 | overlays.rs + store.rs:6385-6393（1000/5000 阈值一致） | components/overlays.rs:24-80 | PORTED |
| MigrationOverlay（v1→v2 迁移进度） | app.tsx:1395；migration-overlay.tsx | **无** | —（grep 无命中） | **MISSING**（NEW，S2；D-06 的 local-state 迁移是另一件事，已修） |
| DevToolsBar（debug.devtools / local channel） | app.tsx:1386-1388；devtools-bar.tsx (433 行) | **无** | — | **MISSING**（KNOWN：auditB §1；dev 向，S2） |
| ErrorBoundary→ErrorComponent | app.tsx:305-310 | 渲染 panic → 错误帧 + 每键退出 | app.rs:5568-5591 | PORTED |
| ThemeErrorToast | app.tsx:383 | boot 时 surface | app.rs:225 | PORTED |
| 终端标题（OSC 0/2，按路由/会话） | app.tsx:596-622 | **无**（terminal_proto.rs 无 `]0;`/`]2;`） | terminal_proto.rs:43-60 | **MISSING**（KNOWN：TM-5，S2） |
| 出口 farewell | mini/splash + presentation | farewell_solid.rs | farewell_solid.rs | PORTED |

## 2. 对话框映射表（ref ~40 面 → rust `DialogKind`）

rust 对话框身份枚举：store.rs:87-222。逐项：

| ref 对话框 | 触发 | rust | 状态 |
|---|---|---|---|
| CommandPalette | ctrl+p | CommandPalette + command_palette.rs（3651 行） | PORTED |
| DialogHelp | /help | Help（dialog.rs 键处理 + palette 分组表 command_palette.rs:1162-1183） | PORTED |
| DialogThemeList | /themes（live 预览/回退） | ThemeList + theme_initial 预览契约 | PORTED |
| DialogModel | /models（变体子对话框、favorite ctrl+f、provider ctrl+a） | ServerList{model.list}（wave B 全套） | PORTED |
| DialogVariant | /variants（无变体 toast 守卫） | Variant | PORTED |
| DialogAgent | /agents | ServerList{agent.list} | PORTED |
| DialogMcp | /mcps（每服务器状态、错误详情、space 开关） | Mcp + McpError | PORTED |
| DialogSessionList | /sessions（**项目分组头、每行 time/model 详情、当前项目标签、ctrl+f 置顶、两击 ctrl+d 删除**） | ServerList{session.list}：**仅标题行**（command_palette.rs:557-576）；pin/两击删除在 store 有（store.rs:3807、dialog.rs:367） | PARTIAL（行详情/分组缺失，#20） |
| DialogOpen | ctrl+o /open（**会话+项目+worktree 分区、loading/empty 态、large**） | `open.menu` → 直接开 Sessions ServerList（command_palette.rs:1805-1817） | **WRONG**（#19，S1） |
| DialogSessionRename | ctrl+r /rename | SessionRename（+shimmer pending） | PORTED |
| DialogWorkspaces | /worktrees、session.move | Worktrees + dialog_worktrees.rs（755 行） | PORTED |
| DialogWorktreeName | 新 worktree（tab 生成 slug） | WorktreeName | PORTED |
| DialogWorkspaceFileChanges | force-remove 确认 | WorktreeDeleteConfirm | PORTED |
| DialogStatus | /status | Status | PORTED |
| DialogDebug | /debug | Debug | PORTED |
| DialogPair | /pair | Pair | PORTED |
| DialogUpdate | /update（7 状态机） | Update + UpdateDialogState（update.rs） | PORTED |
| DialogConfig | /settings | Settings（**本构建可读+可持久化的子集行**：theme/color mode/sidebar/grouping，command_palette.rs:734-745） | PARTIAL（ref 是全量 tabs + 编辑器启动，#23） |
| DialogIntegration（+accounts/method/key/code/pending 5 视图） | /connect | Integration + 5 个 IntegrationXxx 变体（store.rs:186-204） | PORTED（BE-3 host） |
| DialogTimeline | <leader>g | Timeline | PORTED |
| DialogFork | session.fork | ForkPicker | PORTED |
| DialogMessage（消息动作） | 点击用户消息 | MessageActions（含 WorkingDirectory 三动作） | PORTED |
| QueuedPrompts | <leader>q | QueuedPrompts（steer/ctrl+d 删） | PORTED |
| DialogExecute / DialogShellOutput | 工具详情/后台 shell | ToolOutput（SURF-1） | PORTED |
| DialogExportOptions/Result | session.export | SessionExport + ExportResult | PORTED |
| DialogSkill | /skills、prompt.skills | **无**（table-only；autocomplete.rs:100-102 注明 "skills dialog is not ported"） | **MISSING**（#16，S1） |
| DialogStash | prompt.stash/pop/list（prompt/index.tsx:880-913） | **无**（队列 ≠ stash） | **MISSING**（#17，S1） |
| DialogImagePreview | <leader>i prompt.images.view、diff 图像、附件点击（prompt/index.tsx:404） | **无**（`prompt.images.view` 仅键表项 table.rs:692，无 dispatcher） | **MISSING**（#18，S1；图像渲染管线本身已有——image_blocks.rs） |
| DialogExperiments | devtools-bar.tsx:433 | **无**（v2.0.24 实验列表为空数组） | MISSING（S2，等效面为空） |
| DialogErrorDetails（通用） | 各错误行 | 仅 McpError 一个角色 | PARTIAL（#22，S2） |
| ui/dialog-alert、dialog-confirm（通用音调确认） | 插件 API/破坏性询问 | **无**通用件（各删除确认自绘） | MISSING（#21，S2——随 ctx.ui.alert 一起缺） |
| ui/dialog-prompt（通用输入，含 masked） | 插件 API/pair 码 | 仅 rename/worktree/integration-key 特化实例；masked-text.ts 无对应 | PARTIAL（S2） |
| TabContextMenu（右键 tab） | 右键 | TabMenu（含最近关闭变体） | PORTED |
| 通用 select/dialog chrome（60/88/116、backdrop、外点关闭、栈） | ui/dialog.tsx | dialog.rs/dialog_solid.rs | PORTED（wave D 几何对齐） |

## 3. 组件映射表（消息/工具/侧栏/页脚等）

| ref 组件 | rust | 状态 |
|---|---|---|
| 用户消息（文本/文件条 ±N/skill 行/图像条/点击动作） | message/rows.rs + list.rs | PORTED |
| 助手 markdown（流式、语法高亮、表格、OSC8 链接） | message/markdown.rs + syntax.rs（tree-sitter 16 语言，R-01b） | PORTED |
| Reasoning 折叠组（时长后缀、展开 ┃ 槽） | message/rows.rs（wave E1/C） | PORTED |
| 工具组折叠 + **展开逐工具单元格**（bash cmd+4 行折叠输出、edit diff、error 行、hover 展开） | tool_rows.rs + 内联披露（tool: 命中，session.rs:461-475）+ ToolOutput 对话框 | PORTED（SURF-1 修复确认） |
| 子代理卡（spinner/completed/error/Continue）+ SubagentHeading | rows.rs:121-182 / 子代会话视图 | PORTED（wave E2） |
| 压缩消息/压缩排队行/skill 行/interrupted 变体/Revert banner/Loading history | list.rs:336-343,385-422,504-514；rows.rs:916-940；store.rs:1110 | 全部 PORTED（auditB 缺口已闭合） |
| Sidebar（Context tokens/%、MCP、location 行、onboarding 门、plugin 分区） | sidebar.rs + live_sidebar_view（session.rs:888-980） | PORTED，**cost 行 `None`**（session.rs:962，#24） |
| Composer（边框轨道、`╹▀` 过渡、光标、占位池、元数据行、usage `(N%)`、interrupt 双 esc） | composer/{mod,render}.rs + composer_solid.rs；render.rs:66-70 usage | PORTED；缺 shell 模式/live 计数/cost/编辑器上下文（#25-#28） |
| Autocomplete（/命令、@file、@agent、@skill） | composer/autocomplete.rs（fuzzy 复用 dialog_select/fuzzy.rs） | PORTED；**目录补全 recents + ctrl+d 破坏性删除条目缺**（prompt/index.tsx:1958-1985，#29） |
| Command palette（分组、绑定列、suggested） | command_palette.rs（38 条目） | PORTED；palette 可见集合与 ref 有出入（§4） |
| Diff viewer（加载/错误/不可用/空 4 态、文件树、hunk 跳转、split/unified、wrap、单文件、已审标记、来源切换、帮助） | diff_viewer.rs + diff_viewer_mount.rs + DiffSource/DiffHelp（DiffPhase store.rs:1523） | PORTED（SURF-2 修复确认）；**图像 diff 仅文案**（diff_viewer.rs:741,799）、**右键文件菜单（stage/mode）与 diff 导出缺**（#30） |
| Session tabs（水平、未读 •/!、✕/+ hover、attention、+New、右键菜单、重开、历史） | session_tabs.rs + session_tabs_solid.rs + TabHistory（store.rs:262-273） | PORTED |
| Toast（4 变体、action、pending 计数、hover 暂停） | toast.rs | PORTED + EXTRA `toast.activate` 键盘弦（table.rs:792，已登记 PORT ADDITION） |
| Spinner/tab-pulse/marquee/fade-in/shimmer/title-shimmer/one-cell | spinner_frames、tab_pulse.rs、marquee.rs 等 | PORTED（帧预算测试 frame_budget_tests.rs 锁定） |
| TerminalPane（内嵌终端 + <leader>t/down/up 家族 + session-terminals） | **无**（键表注册为吞键 no-op，table.rs:577-581、command_palette.rs:2155+） | **MISSING**（#14，S1） |
| PanelHost 全屏面板（stats/plugins） | **无** | MISSING（#15，S2） |
| PatchDiff | diff 样式族 | PORTED |
| MaskedText | **无** | MISSING（随 dialog-prompt，S2） |

## 4. 键位与 mouse

- **表覆盖**：ref `config/keybind.ts` 231 个 id（含 which-key×12、composer×13）——rust `DEFAULT_TABLE`（oc-tui-core/src/keymap/table.rs:507-832）**全量覆盖**，实测 `comm` diff = ref-only 0。rust-only：`leader`、`app.heap_snapshot`、`composer.close`、`composer.tab.prev/next`、`composer.subagent.toggle-activity`、`toast.activate`（多数为 opentui 键图引擎自带/composer 层内联命令；`toast.activate` 为登记过的端口增补）。
- **leader**：`ctrl+x` + 2000ms（table.rs:13-14）✓；别名展开器（enter/esc/pgup/pgdown）、逗号绑定、backspace-pop、base-layout fallback、escape-clears —— M-engine 移植（ROADMAP 2026-10-15 条目）✓；`gg/dd` 型连击序列语法 ✓（table.rs:214-242）。
- **无表面吞键族**（command_palette.rs:2155-2185）：terminal pane 三件 + pane focus、app.debug/scrap、service.restart、session.aside、session.verbosity.cycle、plugins.install、dialog.integration.*/dialog.plugins.*、composer.terminal.*、which-key.*。其中 **which-key 在 v2 同样无表面**（v2 只注册 6 个 addon，无 which-key 面板，keymap.tsx:4-11——核对确认）→ parity；**terminal pane / verbosity / aside 在 v2 有真实表面** → 端口缺失（见差异表）。
- **mouse**：`?1000 ?1002 ?1003 ?1006`（mouse_modes.rs:10-17，TM-4 已修——1015 已去）；hover/点击命中网格、对话框外点关闭、tab ✕/+ glyph-tight、transcript 点击选择、行移动选择（wave E1）✓；**缺**：选区复制/copy-on-select/右键复制（ref app.tsx:546-571 Selection + `terminal.copy` config；rust app.rs 无任何 selection→clipboard 路径，#31，S1；TM-9 证据空洞仍在）。
- **palette/slash 集合差**：
  - palette：rust 38 条 vs ref 可见集 ~45+（session 路由命令族）——rust 缺 `session.copy`（有 copy.id）、`session.delete`、`messages.copy`（dispatcher 有但 palette 无行）、`prompt.queue`、`prompt.images.view`、`session.queued_prompts`、`session.toggle.scrollbar`、消息导航族、`session.unshare`、`theme.switch_mode/mode.lock`、`terminal.suspend/title.toggle`、`app.toggle.*`（后四类 ref 本就 palette 隐藏，非差异）；rust 多 `toast.activate`、`update.check`。
  - slash：rust `V2_SLASH`（autocomplete.rs:56-91）34 条，**缺 `/share /rename /timeline /fork /compact /undo /redo`**（ref 分别在 session/index.tsx:891-1206 注册 slash；命令本体在 palette 可达）→ **PARTIAL #32（S1：/compact、/undo、/redo、/rename 是高频输入习惯）**。`/cd` 带 arguments 保持 INSERT ✓。
- 键位 override 解析（v1 下划线 id + v2 点 id、array/object 形态、none/false 解绑、坏值跳过+toast）✓（table.rs:334-449）。

## 5. 主题 token 与颜色

- **资产**：29 个 v1 + v2/opencode.json，逐文件 JSON 归一 diff = **0 差异**（字节序不同但语义全等）。
- **发现链**：`<config-root>/themes/*.json` + `.opencode/themes`（theme.rs:450-489，无效文件 log+skip，builtin < custom < system 优先级 theme.rs:314-355）✓；`system` 主题（OSC 11 探测 + 锁定/unlock + `?2027/?2031` 运行时重主题 terminal_proto.rs:51）✓。
- **token schema**（oc-theme/src/types.rs 150 pub 字段）：text/feedback(success|info|warning|error)/action(hovered/focused/selected/secondary/destructive)/formfield(focused/selected)/scrollbar/hue(100-900)/markdown 全族/syntax 全族 —— auditB §11 登记的 6 个 STYLE-MISSING 簇中 success/info（theme.rs:129-132）、formfield、scrollbar 已绑定；`action.secondary.hovered`/`action.primary.hovered`/`destructive.focused` 在 types.rs 有字段，消费面未逐一对扫（标 UNVERIFIED，见 §16）。
- **降级矩阵**：RGB→xterm-256（render/color_mode.rs，termatrix 8 环境 144/144 全对）✓；mini 的 oklab 独立量化器（mini/theme.rs:168）保持 ref 分离实现 ✓。
- v1→v2 迁移（v1_migrate.rs）✓；contrast 继承项（E5）登记不阻塞 ✓。

## 6. 动画 / 帧预算

- 帧预算测试族（frame_budget_tests.rs：tabs/composer/transcript/dialog/sidebar/home/theme 切换）✓；60fps 目标与 settle-loop 对齐（M-wave 语义）。
- 动画开关 `config.animations`、marquee 80ms 真边界（SA-F3）、tab pulse、spinner 帧、fade-in ✓。
- **busy-tab 指示延迟**（rescan F3，S1 KNOWN）：本扫描未复查实时窗口（需 live 双端），代码中 tab 打开时机已走 submit-ack（ROADMAP #12 修复链），标 KNOWN-可能已修-未验证。

## 7. 插件 UI slot（sidebar.content、footer、dialog、router.navigate、keymap.layer）

- ref `ctx.ui` 面（plugin/api.tsx:298-312 slot、:179-199 keymap.layer、router.navigate、dialog、status item、toast、text rows、hint）。
- rust 桥仅：toast / 文本行 sidebar / footer 提示 / dialog（oc-plugin-host/src/host/host.js:698-784）；**`ui.slot`、`ui.router`、`keymap.layer` 全缺** → omo-slim 类插件的 TUI 完全不渲染（rescan F1/F7，**S0 KNOWN**）。
- 原生替代 seam：plugin sidebar 分区（store.plugin_sidebar_sections + SA-F5 版本徽标/白格/列填充）、plugin 命令→palette 行 + owned keymap 和弦（plugin_boot.rs）、plugin dialog materialize-at-open、`tui.command.execute`/`tui.toast.show`/`tui.session.select` 事件面 ✓（plugin_ui.rs/plugin_boot.rs，测试 plugin_slots.rs/vt_plugin_sidebar.rs/vt_plugins_dialog.rs）。
- plugin **路由**（PluginRoute/Slot path="app"）缺（见 §1）。
- F2（插件 MCP 不可见）、F11（注册事件无 payload）为服务端 seam，TUI 表现登记 KNOWN S0/S1。

## 8. 通知 / 声音 / 剪贴板 / 编辑器

| 面 | ref | rust | 状态 |
|---|---|---|---|
| 声音资产（5 个 mp3 + 名称映射） | attention-sounds.node.ts:12-16 | sound.rs:57-70（include_bytes 同资产） | PORTED |
| 播放 | in-process miniaudio | 系统播放器镜像（登记分歧） | KNOWN 分歧 |
| OS 通知 | OSC 9/99/777 | os_notify.rs | PORTED（R-06） |
| attention 触发矩阵 | attention.ts | attention.rs（55+ 行触发表、volume 0.4、per-event 声音覆盖、when 门控） | PORTED |
| 剪贴板写 | OSC 52 all-available | clipboard.rs（100KB 上限） | PORTED |
| 剪贴板读 | OSC 52 query | terminal_proto.rs:240-325（解析回包） | PORTED |
| $EDITOR/VISUAL 外部编辑 | editor.ts | editor.rs（无 fallback 列表、退出码 0 语义一致） | PORTED |
| **Zed 编辑器上下文**（sqlite 选区→editorFileLabel + prompt.editor_context.clear + `/editor` 标签行） | editor-zed.ts、editor.ts:7、prompt/index.tsx:240-250,460,656,1945 | **无**（editor.rs 仅 $EDITOR 流） | **MISSING**（#28，S1——Zed 用户日常面） |
| 选区复制（终端 copy 配置） | app.tsx:546-571 | 无 | MISSING（#31） |

## 9. 终端能力探测与降级

| 项 | 状态 |
|---|---|
| OSC 11 主题模式探测 + 白/黑翻转 | PORTED（termatrix 双向验证） |
| kitty keyboard query/push(`>5u`)/pop | PORTED（app.rs:4498-4823、terminal_proto.rs:43-48；TM-1 已修，且补了 ref 缺的 pop） |
| `?2027h/?2031h` 订阅 + 运行时重主题 | PORTED（TM-2 已修） |
| DECSCUSR + OSC 12 光标色 | PORTED（app.rs:4828-4840；TM-3 已修） |
| mouse 模式集 1000/1002/1003/1006 | PORTED（TM-4 已修） |
| **OSC 0/2 窗口标题** | **MISSING**（TM-5 KNOWN，S2） |
| **sync-2026 帧包裹** | **MISSING**（TM-6 KNOWN，S2；ref 无条件发，我们从不发） |
| SGR-256/truecolor 切换矩阵 | PORTED（8 环境零 diff） |
| DA1/XTVERSION/OSC4 升级探测面 | INFO 级差异（TM-8，无行为差） |
| crash 恢复序列 / raw-mode 回收 | terminal_proto.rs:159-227（含 SA-F13 重入修复） | PORTED+ |

## 10. 流式渲染 / 滚动 / 粘贴 / 多行

- 流式：SSE 增量→transcript 追加、thought 折叠/展开、spinner、`Nms·X.X tok/s` 行 ✓；G11-F1 竞态由证明性测试锁死 ✓。
- 滚动：page/half/line/first/last（f64 半页 + js_round）、viewport=transcript−1、跳底槽、软换行硬折行 ✓（wave E1/C）。
- 粘贴：bracketed paste（app.rs:4434,5724）、路径→附件芯片（R-07）、粘贴摘要开关（app.toggle.paste_summary 命令在表）✓；多行 draft 硬换行 ✓（F6 修复链）。
- 队列：`<leader>return` 入队、队列 dock 行、steer/删除 ✓（app.rs:1767-1912）。

## 11. 错误 / 空状态

- 对话框 loading/error/empty 三态（ServerList/Integration/Worktrees/Diff 四态）✓；MCP failed/needs_auth toast（store.rs:4905-4922，含"Open MCP servers"动作）✓；session 删除通知、send-failure 错误面板（F11 修复：titled 模态直到关闭）✓；ErrorComponent ✓；ThemeErrorToast ✓；config 坏值→toast（log_skip→drain）✓。
- **缺**：SessionLocationMissing 独立屏（ref routes/session/location-missing.tsx——目录失效恢复路径；rust 中 location 失效走 toast/工作目录对话框，#33 未深扫，标 PARTIAL/UNVERIFIED）。

## 12. 搜索 / 过滤

- 通用 fuzzy 选择器（dialog_select/fuzzy.rs + 语料库 fuzzy_corpus.json）✓。
- **session 列表过滤语义错位**（SPEC-R6 E12，KNOWN，6 项）：F1 子会话泄漏、F2 重音等价、F3 NFD、F4 子串 vs fuzzy（rust 走共享 fuzzy、ref 是专用 substring+server-search）、F5 空白折叠、F6 空查询返回空而非全量 —— 待 LIVE ref 复核后逐项裁决（FINDINGS.md §SPEC-R6）。
- palette/models 过滤（61 截断、全宽回显）✓（wave B）。

## 13. mini 子系统（`opencode mini`）

| ref mini | rust | 状态 |
|---|---|---|
| runtime{,.queue,.lifecycle,.boot}、transport(stream-v2)、scrollback{.writer,.surface}、splash、theme(oklab)、mono、demo、prompt_shared、session/model 共享、verbosity、variant | mini/{runtime,queue,lifecycle,transport,scrollback,splash,theme,mono,demo,prompt_shared,model_pref}.rs | PORTED（R-02，PTY 测试类） |
| footer 家族（view/prompt/form/command/permission/menu/width） | components/footer/{mod,form,permission,panels,panel,canvas,prompt_idle,replay,form_authorization}.rs（mini footer 为共享源） | PORTED |
| turn-summary、entry.body | mini/scrollback.rs | PORTED |
| **stream-v2.subagent.ts + footer.subagent.tsx（子代理 inspector，14 行视图）** | **无**（mini/ 内仅 transport.rs:349 的工具名归一；子代理 UI 只在主 TUI session 路由有） | **MISSING**（#34，S1——mini 下跑子代理时页脚无运行态/导航） |
| stream-v2.image | image.rs/image_blocks.rs（主 crate） | PORTED（R-07/08） |
| kitty flags 启停（buildKittyKeyboardFlags） | lifecycle.rs + terminal_proto | PORTED |

## 14. 差异总表（EXPECTED vs ACTUAL）

> 格式：# | 级别/分类/登记 | ref（EXPECTED） | rust（ACTUAL） | 影响

| # | 级/类/登记 | EXPECTED（ref） | ACTUAL（rust） | 影响 |
|---|---|---|---|---|
| 1 | S0 MISSING KNOWN(rescan F1/F7) | 插件经 `ctx.ui.slot("sidebar.content")` 等渲染自有 UI（plugin/api.tsx:298-312） | host.js:698-784 仅 toast/文本行/footer 提示/dialog；omo-slim 类 TUI 完全不出现 | 旗舰插件用户侧栏/交互全灭（rescan Top-1） |
| 2 | S0 MISSING KNOWN(rescan F2) | 插件注册的 MCP 服务器出现在 sidebar/DialogMcp（feature-plugins/sidebar/mcp.tsx） | 引擎 seam 不转发 Registrations.mcps | 插件 MCP 不可见 |
| 3 | S1 MISSING NEW | 垂直 tab 布局 + 拖宽 + `tabs.layout` config（app.tsx:574-585,1347-1349） | 仅水平 strip（session_screen.rs:270） | 宽屏用户布局选择缺失 |
| 4 | S1 MISSING NEW | TerminalPane + `<leader>t/down/up` + terminals tab（terminal-pane.tsx；session-terminals.tsx） | 键表注册吞键 no-op（table.rs:577-581） | 内嵌终端整面缺失 |
| 5 | S1 WRONG NEW | `open.menu`(ctrl+o) → DialogOpen：最近会话+项目+worktree 分区、loading/empty（dialog-open.tsx） | 直接复用 Sessions ServerList（command_palette.rs:1805-1817） | 项目切换半缺失；标题与分区文案不同 |
| 6 | S1 PARTIAL NEW | DialogSessionList 行：项目分组头、time/model 详情、当前项目标签（dialog-session-list.tsx） | 仅标题行（command_palette.rs:557-576） | 长列表可用性下降 |
| 7 | S1 MISSING KNOWN(E12 F1-F6) | 会话过滤 = trim+case-fold 子串、父排除、空查询全量 | 共享 fuzzy：重音/NFD/空白折叠、子会话泄漏、空查询空集 | 搜索结果集不同（6 子项见 FINDINGS） |
| 8 | S1 MISSING NEW | DialogSkill `/skills`（prompt/index.tsx:581-583；dialog-skill.tsx） | 无 dispatcher 入口（autocomplete.rs:100-102） | `/skills` 选中后无表面 |
| 9 | S1 MISSING NEW | DialogStash（stash/pop/list，prompt/index.tsx:880-913） | 无（草稿 stash 仅内部 per-session draft） | stash 工作流缺失 |
| 10 | S1 MISSING NEW | DialogImagePreview `<leader>i` / 附件点击 / diff 图像（prompt/index.tsx:404；diff-viewer-image.tsx:50） | 键表-only（table.rs:692）；diff 图像仅占位文案（diff_viewer.rs:741,799） | 图像查看交互缺失（渲染管线已备） |
| 11 | S1 MISSING NEW | slash `/share /rename /timeline /fork /compact /undo /redo`（session/index.tsx:891-1206） | V2_SLASH 34 条不含此 7（autocomplete.rs:56-91） | 高频斜杠习惯断裂（palette 可达） |
| 12 | S1 MISSING NEW | composer footer live `N subagents · M shells`（hover→session.child.first）（feature-plugins/prompt/footer.tsx:15-45） | 无该段（render.rs 只有 usage/interrupt） | 运行态可见性下降 |
| 13 | S1 MISSING NEW | cost `$X.XX`（footer.tsx:44 + sidebar context.tsx:37） | sidebar `cost: None`（session.rs:962） | 花费不可见 |
| 14 | S1 MISSING NEW | shell 模式 `!` 前缀（独立 footer/提交语义，prompt/index.tsx:991-1024） | 无 | shell 问答模式缺失 |
| 15 | S1 MISSING NEW | Zed 编辑器上下文标签 + `prompt.editor_context.clear`（editor-zed.ts；prompt/index.tsx:240-250,1945） | editor.rs 仅 $EDITOR | Zed 用户上下文注入缺失 |
| 16 | S1 MISSING KNOWN(TM-9 邻接) | 选区复制/copy-on-select/右键复制 + `terminal.copy` config（app.tsx:546-571） | 无 selection→clipboard 路径 | 鼠标复制工作流缺失 |
| 17 | S1 MISSING NEW | mini 子代理 inspector（stream-v2.subagent.ts + footer.subagent.tsx） | mini/ 无对应（仅 transport 工具名归一） | mini 下子代理无运行态页脚 |
| 18 | S2 MISSING KNOWN(TM-5) | 终端标题 OSC 0（app.tsx:596-622） | 从不设置 | 窗口/tab 名缺失 |
| 19 | S2 MISSING KNOWN(TM-6) | 每帧 sync-2026 包裹（ref 无条件） | 从不包裹 | 慢终端闪烁差 |
| 20 | S2 MISSING KNOWN(auditB) | DevToolsBar（app.tsx:1386-1388） | 无 | dev 向 |
| 21 | S2 MISSING NEW | MigrationOverlay（app.tsx:1395） | 无 | v1→v2 迁移进度 UI 缺 |
| 22 | S2 MISSING NEW | plugin 路由 + `OPENCODE_ROUTE`/`OPENCODE_STORY`（app.tsx:337-352）+ stats/plugins 面板 + storybook | Route 仅 Home/Session（store.rs:44-49） | 插件全屏路由不可达（dev 面板同缺） |
| 23 | S2 PARTIAL NEW | DialogConfig 全量 tabs + 编辑器启动（dialog-config.tsx） | Settings 仅 4 行子集（command_palette.rs:734-745） | 设置面窄 |
| 24 | S2 PARTIAL NEW | DialogErrorDetails 通用（dialog-error-details.tsx） | 仅 McpError 角色 | 错误深挖面窄 |
| 25 | S2 MISSING NEW | ui/dialog-alert + dialog-confirm 通用音调确认（含 ctx.ui.alert/confirm） | 各删除确认自绘、无通用件 | 插件确认 API 缺 |
| 26 | S2 MISSING NEW | masked-text（pair 码输入等） | 无 | 遮蔽输入缺 |
| 27 | S2 PARTIAL NEW | diff 右键文件菜单（stage/mode，diff-viewer-file-menu.tsx）+ diff 导出 | 无菜单/导出 | diff 操作窄 |
| 28 | S2 PARTIAL UNVERIFIED | 目录补全 recents + ctrl+d 破坏性删除条目（prompt/index.tsx:1958-1985） | autocomplete 有 @file/@agent/@skill；recents/destructive 未见 | 补全深度差 |
| 29 | S2 MISSING NEW | `app.console` 渲染器控制台切换（含 ctrl+y copy-selection，app.tsx:242-244,1110-1118） | palette 行存在、无控制台表面 | 调试向 |
| 30 | S2 PARTIAL UNVERIFIED | SessionLocationMissing 屏（location-missing.tsx） | 未见独立屏 | 恢复路径可能弱化 |
| 31 | S2 EXTRA | —（v2 toast 动作仅指针） | `toast.activate` ctrl+alt+t 键盘弦（table.rs:792，登记 PORT ADDITION） | 已登记增补 |
| 32 | S2 WRONG KNOWN(rescan F11) | 插件注册事件带 payload（command/agent/mcp.updated） | 空帧（oc-server announce.rs:70-80） | 客户端反应式发现受限（服务端 seam） |
| 33 | S2 PARTIAL UNVERIFIED | `session.verbosity.cycle`（低细节度切换，index.tsx:1065） | 吞键 no-op（command_palette.rs:2169） | 冗长控制缺 |
| 34 | S2 MISSING KNOWN(auditB #?) | palette 缺 `messages.copy`/`session.copy`/`prompt.queue`/`session.queued_prompts`/消息导航族/prompt.images.view 行（app.tsx:688-1216 + index.tsx 命令） | 38 条（command_palette.rs:36-308）；部分命令 dispatcher 可达但无 palette 行 | 可发现性差 |

## 15. KNOWN/NEW 对照（与既有文档）

| 来源文档 | 条目 | 本次判定 |
|---|---|---|
| ROADMAP §4.5 / changelog | 47 项差分账本 | 全部关闭/归类（KNOWN，已验证代码侧闭合：skill 行/压缩/revert/interrupted/loading-history/sidebar live/usage% 等） |
| docs/rescan-findings.md | F1/F2/F7（S0 插件 UI）、F3（busy 指示）、F4（usage%）、F11 | F1/F2/F7 仍开放；F4 已修（render.rs:66-70 + sidebar percent）；F3 未复查（标 UNVERIFIED）；F11 服务端 seam 仍开 |
| docs/termatrix-findings.md | TM-1..TM-10 | TM-1/2/3/4/7(部分) 已修（terminal_proto.rs/app.rs）；TM-5/6 仍缺；TM-8/9/10 INFO |
| FINDINGS.md SPEC-R6 | E12 六项 | 全部仍开放（代码未动，#7） |
| docs/auditB-surfaces.md | 12 STUB + 24 MISSING | STUB 清零（仅 fallback 臂留 "Not wired"）；MISSING 中 21 项已修（附录 A），余 ~10 项并入本表 |
| docs/auditA-interactions.md | 键位/鼠标族 | 与 §4 结论一致（表全覆盖；mouse selection 缺） |

## 16. 覆盖清单（已核对 / 未覆盖）

**已核对（方法）**：路由枚举与挂载（store.rs:44-49 vs app.tsx Switch）；DialogKind 全枚举 vs ref component/ui 对话框清单逐个 grep 验证；键位表全量 id diff（comm 231/231）；palette/slash 表 diff；主题资产逐文件 JSON diff + types.rs token 键盘点；终端协议序列清单（terminal_proto.rs 常量 vs termatrix 报告）；sidebar/composer/message/tool_rows/diff_viewer/mini 模块文档头+关键函数抽读；早期审计条目逐条重验（grep 当前代码）；git log 校验修复 lane 落地。

**未覆盖/待深挖（诚实声明）**：① 会话路由 3591 行与 rust store.rs 的逐行为级 diff（anchors.ts 导航粒度、mount-budget、activity-summary 细节未逐行核对）；② Form/Permission 展示体深度（diff 预览、reject 自由文本阶段）未逐字段核对；③ 主题 token 的“消费面”绑定（secondary.hovered/destructive.focused 等）未逐组件扫描；④ 双端 live 差分未运行（本扫描为静态）；⑤ mini footer 宽度截断规则数值级比对未做；⑥ frecency/目录 recents 数据结构细节未比。

## 附录 A：auditB 时代登记、现已修复确认的条目（21）

Reconnecting/StartupLoading/ErrorComponent/ThemeErrorToast；sidebar live（context tokens+percent、MCP、location、onboarding 门控+✕）；MCP needs_auth/failed toast；home 更新 banner + MCP cell；DialogUpdate；DialogSkill→（未修，除外）；工具展开单元格+DialogExecute/ShellOutput；compaction/skill/interrupted/revert/loading-history；jump-to-latest info 色；success/info token；diff viewer 挂载+四态+来源切换；kitty push、DECSCUSR/OSC12、?2027/2031、mouse 模式集；usage `(N%)` footer + sidebar percent；`/help` 几何族（wave D）；palette "Not wired" stub 清零（dialog 家族实质化）；两击删除；tab 上下文菜单；farewell 出口屏。

## 附录 B：文件级映射索引（速查）

| ref | rust |
|---|---|
| app.tsx | app.rs（9002 行，run loop/键泵/鼠标/协议） |
| routes/home.tsx | components/home.rs + home_solid.rs |
| routes/session/index.tsx | components/session.rs + session_screen.rs + store.rs 会话切片 |
| routes/session/sidebar.tsx | components/sidebar.rs + sidebar_solid.rs |
| routes/session/composer/* | components/composer/* + picker_solid.rs + subagent_picker.rs |
| component/prompt/* | components/composer/* + composer_solid.rs |
| routes/session/{rows,message-parts,group-view}.tsx | components/message/* + tool_rows.rs |
| component/session-tabs*.tsx | components/session_tabs*.rs + store tabs 模型 |
| component/dialog-*.tsx | store.rs DialogKind + dialog.rs/dialog_solid.rs/dialog_select.rs/command_palette.rs props |
| feature-plugins/system/diff-viewer* | components/diff_viewer.rs + diff_viewer_mount.rs |
| feature-plugins/system/notifications.ts | notifications_plugin.rs + os_notify.rs |
| ui/{dialog,toast,animation,spinner,layout,border}.ts* | components/dialog*.rs、toast.rs、theme.rs、layout.rs |
| context/{keymap,route,theme,data,...}.tsx | oc-tui-core/keymap/*、store.rs、data.rs、theme.rs |
| config/keybind.ts | oc-tui-core/src/keymap/table.rs |
| theme/** | oc-theme/** + oc-tui/src/theme.rs |
| mini/** | oc-tui/src/mini/** + components/footer/** |
| plugin/** | plugin_boot.rs + plugin_ui.rs + oc-plugin-host host.js |
| attention*.ts / audio.ts | attention.rs + sound.rs |
| clipboard.ts / editor*.ts | clipboard.rs / editor.rs |
