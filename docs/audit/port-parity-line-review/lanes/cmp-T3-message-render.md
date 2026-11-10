# T3 结论: oc-tui 会话页/消息渲染域为高保真逐行移植（分组投影、part 渲染、diff-viewer、tabs/sidebar/chrome 均有 file:line 级对应 + rust 独有 golden 验证设施），但存在 1 处确定性格式化 bug、若干 low-verbosity/调试面/交互面的有界偏差，语法高亮覆盖面默认构建窄于 ref。

## 对比表

### A. part / 消息类型逐项（ref session/index.tsx + message-parts.tsx ↔ rust message/{model,list,rows,tool_detail}.rs + tool_rows.rs）

| 类型 | ref 位置 | rust 位置 | 判定 |
|---|---|---|---|
| text | message-parts.tsx:169-196（markdown，paddingLeft 3，conceal，streaming） | list.rs:968-1006 `text_part_row`/`source_text_part_row`（`session.markdown=source`）+ markdown.rs（opentui-core pipeline） | ✅ 对齐（markdown 走移植的 MarkdownRenderable，含 mermaid/latex seam） |
| reasoning | message-parts.tsx:42-167（hide 模式单行 + 展开体，warning 0.6 tint，`[REDACTED]` 剔除） | rows.rs:196-282；model.rs:896 `reasoning_summary`；REDACTED 剔除在 store 投影 | ✅ 对齐；缺 `· N steps`（多 thought 计数）→ D-6 |
| 工具行通用（InlineToolRow） | message-parts.tsx:198-291（icon 2 宽槽、spinner/pending/complete/denied 删除线/error 展开） | rows.rs:86-119；denied 删除线与 permission-warning 色未见对应 | ✅ 布局对齐；permission 黄色高亮/删除线/hover 交互部分缺（D-8） |
| shell 工具 | index.tsx:2780-2952（BlockTool + 后台 shell 1s 轮询、1MiB cap、collapseShellOutput） | tool_detail.rs:239-329 + list.rs:630-677（shell_live seam）；collapse 逐行移植 | ✅；运行中命令 ref 用 text.base、rust 用 muted（D-15） |
| glob/grep/read | index.tsx:2990-3052（含 matches/count、`↳ Loaded` 行） | tool_detail.rs detail cells | ✅ |
| webfetch/websearch | index.tsx:3054-3085（RetryProvider 动画） | tool_detail.rs 有行渲染；RetryProvider 动画 span 无对应 | ⚠️ D-14 |
| write/edit/patch | index.tsx:2954-2988/3240-3422（BlockTool + PatchDiff + Diagnostics） | tool_detail.rs:162-450+（`parse_apply_patch_files`、`patch_hunk_rows`、delete→`-N lines`）；Diagnostics 行未见 | ✅ 主体；Diagnostics(LSP error 行)未见 → D-13 |
| subagent | index.tsx:3087-3133（continuation ↳、Background 徽章、点击跳转、模型目录名） | rows.rs:121-184 + list.rs:851-863 | ✅ 主体；模型后缀用原始串（ref 查目录+`#variant`）、Background 徽章/点击跳转缺 → D-5 |
| execute | index.tsx:3196-3238（子调用列表 + 错误预览 + DialogExecute） | tool_detail.rs execute 卡片 + command_palette.rs:744；嵌套调用计数偏差（自注） | ✅（有界偏差已声明） |
| question/skill | index.tsx:3424-3467 | tool_detail.rs + rows.rs:907-913 | ✅ |
| user 消息 | index.tsx:2185-2310（raised 盒 + agent 色 rail + skills/files 徽章 + 图片条） | rows.rs:531-598 + `image_strip_row`（R-07：kitty/sixel/blocks 回退齐全） | ✅；skills/files 徽章行、pending-delivery 时 rail 变 border.base 未见 → D-11 |
| compaction（含 queued） | index.tsx:2002-2081 | rows.rs:772-905 + list.rs:453-459 | ✅ |
| idle / skill / synthetic / shell(!) / agent/model/location-switched / system | index.tsx:1782-2310 | model.rs:262-361 + rows.rs:907-1054 + list.rs:691-827 | ✅（空 description synthetic 见 R-2） |
| assistant footer（error/interrupted/retry/tps/宽度门控） | index.tsx:1852-1901, 2344-2369 | rows.rs:600-757（宽度门 28/36、interrupted、retry 倒数全对齐） | ✅；tps 口径不同 → D-12 |
| snapshot/cost/tokens/finish | footer usage/`TurnTokenUsage` | list.rs:510-563 摘要行 + TranscriptOptions.turn_tokens | ⚠️ 展开表/缓存击穿警告未移植（自注有界偏差）→ D-4 |

### B. 分组 / 虚拟化 / 导航

| 项 | ref | rust | 判定 |
|---|---|---|---|
| grouping/tree（groupEntries/merge/split） | tree.ts:17-111 | model.rs:750-843 `project_entries` | ✅ 算法等价 |
| grouping/session（partPath/messagePath/pending 分区） | session.ts:78-184 | model.rs:586-629, 853-891 | ⚠️ D-1/D-2/D-3 |
| rows.ts reduce + SSE 增量 append | rows.ts:47-411 | model.rs `reduce_rows_with_rules`（全量重算）+ store.rs SSE | ✅ 形状等价 |
| mount-budget（rowWeight 权重） | mount-budget.ts:10-38 | list.rs:586-620（**flat 权重**） | ⚠️ D-16/R-3 |
| anchors/message-navigation | anchors.ts / message-navigation.ts | store.rs:7593-7850 + dialog.rs:1732 timeline | ✅ |
| activity-summary | activity-summary.ts | model.rs:1023-1099 + list.rs:1028-1077 | ✅（execute 嵌套计数偏差已自注）；running 纯-thought 组标签缺失 → R-5 |

### C. 组件逐个

| 组件 | ref | rust | 判定 |
|---|---|---|---|
| session-frame/装配 | session-frame.tsx + index.tsx | session_screen.rs + session.rs | ✅ |
| sidebar（context/mcp/footer 插件槽） | sidebar.tsx + feature-plugins/sidebar/* | sidebar.rs + sidebar_solid.rs（title_shimmer 两相动画、scrim） | ✅ |
| permission 提示（两段式/reject 自由文本/EditBody diff 预览） | permission.tsx（622L） | footer/state.rs:476-500+ + footer/permission.rs | ✅ |
| form.tsx | routes/session/form.tsx | footer/form.rs + form_authorization.rs | ✅ |
| diff-viewer 全屏（树/单 patch/分栏↔统一/图片/hunk 跳转/mark_reviewed/help） | diff-viewer*.tsx + file-tree-utils | diff_viewer.rs + diff_viewer_solid.rs + diff_viewer_mount.rs + `split_added_patch`（G-130） | ✅ |
| patch-diff 行内 | component/patch-diff.tsx | tool_detail `patch_hunk_rows` + solid DiffRenderable gutter sync | ✅ |
| session-tabs/rail | session-tabs.tsx/-rail.tsx | session_tabs.rs(+solid/marquee)/session_tabs_rail.rs — **golden ANSI 字节级** | ✅ |
| terminal-pane | terminal-pane.tsx | terminal_pane.rs + terminal_pane_solid.rs | ✅ |
| spinner 体系 | spinner.tsx/frames/register/one-cell | rows.rs:32-49（10 帧 braille@80ms，帧表与 ref 完全一致）+ tab_pulse 等 | ✅ 帧表；one-cell/register 通用注册面未见独立对应 → D-17 |
| startup-loading/logo/reconnecting/use-connected | 各文件 | chrome_solid.rs + overlays.rs（1s/5s 重连时序照抄 app.tsx）+ home.rs logo | ✅ |
| retry-provider | retry-provider.tsx | 无对应（静态 provider 名） | ❌ D-14 |
| masked/shimmer/fade-in/title-shimmer | 4 文件 | masked_text.rs + sidebar_solid + dialog shimmer | ✅ |
| error-component/migration-overlay/theme-error-toast/plugin-route-missing | 各文件 | update.rs + migration_overlay.rs（全对齐）+ chrome_solid 兜底 | ✅ |
| panel-host / tab-pulse | panel-host.tsx / tab-pulse.tsx | footer/panel.rs+panels.rs / tab_pulse.rs | ✅ |
| stats 页 | stats.tsx+stats-data.ts+activity-calendar | stats_page.rs（M-W-S 日历、大数字位图）+ stats_page_solid.rs | ✅ |
| notifications/plugins feature-plugins | notifications.ts/plugins.tsx | notifications_plugin.rs（触发/声音表逐条对照）+ store.rs:1254+ | ✅ |
| home | home.tsx | home.rs + home_solid.rs（golden 字节级） | ✅ |
| dialog-{execute,fork,message,timeline} | session/dialog-*.tsx | dialog.rs:1732-1922（fork 语义逐条注释对齐） | ✅ |
| markdown 渲染管线 | plugin/markdown.ts（last-wins 注册） | message/markdown.rs（opentui-core 管线，mermaid/latex 内建 seam） | ✅ 渲染；**插件自定义代码块渲染器注册面无通用对应** → D-10 |
| 语法高亮 | parsers-config.ts：35 filetype wasm 运行时从 GitHub 拉 | oc-syntax 原生 vendor：默认 20（+`langs-extra` 15、csharp/kotlin/swift 独立 feature） | ⚠️ D-7（默认构建无 bash/csharp/kotlin/swift/vue 等；markdown 无 injection 降级） |
| golden 测试机制 | 无对应 | golden_tests.rs + 4 文件（text-grid+canonical ANSI 字节比对） | ➕ rust 独有验证设施 |

## 行为差异清单 D-n

- **D-1** `messagePath` 低 verbosity 缺 `["activity","instructions"]` 包裹（ref grouping/session.ts:94-97 ↔ rust model.rs:623-629 仅 `[Instructions]`）→ low 档 instruction 载入不进 activity 汇总、组内 instruction 文件计数恒 0。
- **D-2** `partPath`：ref 中 `question` 工具永远独立成行；rust 低 verbosity 会把 question 包进 Activity。
- **D-3** pending 输入重排：ref 将 pending inputs/compaction 移到行尾（rows.ts:373-377）；rust 保持原序仅置 `closes_previous`。
- **D-4** `TurnTokenUsage` 仅移植折叠摘要行；展开 per-step 表、cache-bust 警告未移植；"turn 结束"判定用"下一行是 User"，与 ref 的 idle-marker/legacy 语义不完全等价。
- **D-5** subagent 卡：模型后缀为原始 `input.model` 串（ref 解析目录名+variant）；`Background` 徽章、点击跳子会话、`metadata.sessionId` 回退均缺。
- **D-6** reasoning 组头缺 `· {N} steps`。
- **D-7** 语法高亮覆盖：ref 35 wasm（含 bash/csharp/kotlin/swift/vue/hcl/haskell/julia/ocaml/clojure/nix/elixir/fsharp/r/xml/agda）；rust 默认 20 原生，其余在非默认 feature；markdown 无 injection。反向优势：离线、确定性。
- **D-8** InlineTool 的 permission-warning 前景、denied 删除线、error hover 变色在 headless 渲染器未建模（solid 侧部分补齐）。
- **D-10** 插件注册 markdown 代码块渲染器（last-wins 合并）无通用开放面。
- **D-11** user 消息块：skills/files 徽章行与 pending-delivery 时 rail 改 `border.base` 未移植；rust rail 恒为 agent 色。
- **D-12** tok/s 口径：rust = 末步 tokens/(completed−created)；ref = 回合内各步 output+reasoning 之和 ÷ 各步 streamed 时长和。
- **D-13** `Diagnostics`（write/edit 后 LSP error 行）未见对应。
- **D-14** `RetryProvider`（websearch 行 provider 名渐变动画）无对应。
- **D-15** 运行中 shell 命令前景色：ref `text.base`/permission 黄；rust muted。
- **D-16** mount 预算：rust flat 权重（每行 1）；ref `rowWeight` 展开组按叶子数计。
- **D-17** one-cell-spinner/register-spinner 通用注册体系未见独立对应（帧表本身一致）。
- **D-18** `turn_duration` 回溯输入只认 User；ref `inputIndex` 还认 synthetic。
- ➕ **D-19** golden 帧测试设施为 rust 独有。

## 正确性风险 R-n

- **R-1（高置信 bug）** `locale_duration` 秒档取模错误：`milliseconds_to_seconds = f64::from(input as u32 % 10_000)/1000.0`（model.rs:915-918, 926）。10s–60s 区间时长显示错误：15000ms → `"5.0s"`（ref locale.ts:29 为 `"15.0s"`）；59500ms → `"9.5s"`。影响 assistant footer 时长、Thought 时长、组时长。应删 `% 10_000`。
- **R-2** 空 description 的 synthetic 消息渲染裸 `◈` 空行：ref 在 reduce 时整体跳过（rows.ts:383）；rust 不跳过且仍画图标。
- **R-3** 虚拟化窗口与展开组不一致（D-16 后果）：flat 权重使 TAIL_ROWS=40/CHUNK=60 预算失真（欠挂载/过挂载）。
- **R-4** turn-usage 摘要触发启发式（footer 后继为 User 才结算）：idle 标记后跟非 User 行或 steered 回合无新 User 时摘要错位/漏出。
- **R-5** 低 verbosity 下仅含运行中 thought 的 activity 组渲染为空：rust 无 ref 的 `"Thinking…"` busyLabel 回退 → 组行消失。
- **R-6（未验证疑似）** subagent 运行判定的 streaming 期 `metadata.sessionID` 时序与 ref `session.status` 完全一致性需 store 喂 `subagent_running` 验证。

## 覆盖声明

1. **双侧全文逐行**：ref routes/session/{index.tsx(3591), message-parts, group-view, grouping/{session,tree}, rows, sidebar, anchors, activity-summary, mount-budget, thinking-syntax, message-navigation, permission(部分), plugin/markdown.ts}；rust message/{model,list,rows,tool_detail}（markdown.rs 头部+结构）、model.rs 全文含测试。
2. **结构性核验**：rust sidebar(_solid)、session_screen/session_tabs(+solid/rail)、diff_viewer(+solid/mount)、chrome_solid、overlays、home、footer 家族、stats_page、toast、tab_pulse、masked_text、migration_overlay、location_missing、update、terminal_pane_solid、tree_capture、oc-syntax lib、store.rs 导航/权限/滚动锚段；ref parsers-config（filetype 全表）、permission.tsx 前 180 行+结构 grep、spinner-frames/retry-provider、util/locale。
3. **仅清单级（未逐行）**：ref composer/*、dialog-{execute,fork,message,timeline} 全文、home.tsx、session-frame/session-tabs/terminal-pane/patch-diff/one-cell-spinner/startup-loading 正文、feature-plugins/system/{stats,plugins,notifications,file-tree*} 正文、util/diff.ts；rust composer.rs/composer_solid.rs、command_palette.rs、session.rs 主体、dialog.rs 主体、diff_viewer_solid.rs、golden_tests.rs 正文（只读机制）。
4. **未覆盖**：rust store.rs 的 SSE 增量 append/reconcile 与 ref createSessionRows 逐事件对拍；solid 画笔与 headless walk 的逐像素一致性（依赖 golden）；history prepend（ref history.ts）在 rust 的对应实现位置未定位。
