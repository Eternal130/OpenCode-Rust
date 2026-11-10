# L15 TUI 组件层 — 审查报告

> 审查方式：两侧文件全量清单 + 函数级索引逐条比对 + 关键实现段全文阅读；Rust 注释带 ref file:line 引用已抽样回验。未发现"注释声称已对齐但代码缺失"的情况。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明与分类

**方法**：两侧全量文件清单已取得（ref component 18,042 行 + rust components 64,995 行）；对 syntax.rs、image_view 骨架、composer/autocomplete、dialog.rs、footer/mod、scroll.rs、animation.rs 做了全文阅读；其余文件按函数级索引逐条比对并抽查关键实现段。

**components/ 分类**（Rust 侧）：
- **应用组件**（逐行对照 ref）：`composer/*`+`composer_solid`（←component/prompt/* + prompt/* + routes/session/composer/*）、`message/*`+`tool_rows`+`transcript_solid`（←routes/session/{message-parts,rows,group-view,grouping}）、`dialog*`+`toast*`+`command_palette`+`subagent_picker`+`overlays*`+`update`+`migration_overlay`（←ui/* + component/dialog-*）、`session*`+`sidebar*`+`home*`+`footer/*`（←routes/session + mini/footer*）、`tab_pulse`、`masked_text`、`devtools*`、`plugin_page/slots`、`stats_page`、`console*`、`location_missing*`、`terminal_pane_solid`、`syntax.rs`（←parsers-config + util/filetype）。
- **框架组件**（只对照应用可见行为）：markdown 渲染内核（ref 用 @opentui/markdown 内建，Rust 在 `message/markdown.rs` 自研等效渲染器）、图片管线（`image_view.rs`/`image_codec.rs` 对标 opentui drawImage/sixel/kitty/blocks）、text_edit（oc-tui-core）对标 opentui TextInput 编辑命令。
- 测试基建（golden_tests、replay 等）为 🔵 移植版新增。

### 2. 逐项对比表

| 功能点 | Ref 实现 | Rust 实现 | 状态 | 差异说明 |
|---|---|---|---|---|
| markdown→组件树（标题/列表/引用/hr/表格/段落边距） | plugin/markdown.ts:1-18（opentui 内建） | message/markdown.rs:230-545 | ✅ | 自研渲染器，测试钉住布局/换行/表格网格（:945-1063） |
| 链接（OSC8 支持时隐藏 URL 尾巴） | opentui 内建 | markdown.rs:875-931 | ✅ | 双态（支持/不支持超链）均有测试 |
| 代码块高亮 | parsers-config.ts（34 filetype） | syntax.rs:366 + oc-syntax/queries.rs（39 filetype） | ✅ | ref 34 个 filetype 全覆盖（REF_ONLY 为空）；js/ts/md 走 opentui 内建→Rust 显式注册。filetype_for_path（syntax.rs:399）仅映射 20 扩展名，比 ref LANGUAGE_EXTENSIONS 窄 |
| injections 查询 | parsers-config.ts:154-156（HTML injections 被注释掉） | syntax.rs:73 `injections_query: None` | ✅ | ref 本身也未启用 injections，语义一致（markdown fence 内嵌语言纯文本） |
| diff 着色/虚拟化 | component/patch-diff.tsx:37-123, util/diff.ts | diff_viewer.rs:61-345（G-130 逐条引 ref 行号） | ✅ | splitAddedPatch/VIRTUAL_CHUNK_LINES 常量与分块算法对齐 |
| 工具调用卡片（图标/折叠/输出截断） | message-parts.tsx:21-90,198-288；util/collapse-tool-output.ts | tool_detail.rs:36-148, rows.rs:92-195 | ✅ | collapseShellOutput(10, 10·max(20,w−6)) 参数逐字对齐 |
| 思考块（dim 前景色语法） | thinking-syntax.ts:3-7 | message/markdown.rs 思考样式 + rows.rs:196-243 | ✅ | Thinking 标题/条目行渲染齐全 |
| 图片（kitty/sixel/blocks 回退） | opentui drawImage | image_view.rs:65-307 | ✅ | 协议探测、PNG passthrough、occlusion 检测均有 |
| 子代理消息/缩进 | rows.ts/grouping/tree.ts | rows.rs:139 (subagent_tool_row), message/list.rs | ✅ | |
| 消息 footer/错误/压缩/技能/切换行 | rows.ts:538 全部行型 | rows.rs:610-1057（14 种行型逐一实现） | ✅ | |
| 多行编辑（光标/选区/undo/词移动） | opentui TextInput | oc-tui-core/text_edit.rs:291-842 | ✅ | apply_tracked/undo 栈/word-boundary 齐全 |
| composer 芯片模型（mention/粘贴占位） | prompt/index.tsx:1397-1477 | composer/mod.rs:186,890-1187 | ✅ | expand_pasted_ranges、chip 前后光标语义对齐 |
| @补全（file/agent/skill、frecency 排序） | prompt/frecency.tsx:1-80, mention.ts | composer/autocomplete.rs:372-375,598-726 | ✅ | score×(1+frecency) 与 DirectoryRecents 对齐 |
| 目录补全/最近目录 | directory-completion.ts:1-98, directory-recents.ts | autocomplete.rs:430,714-726 | ✅ | |
| 斜杠命令补全（含 destructive 二次确认） | prompt/autocomplete.tsx:953 行 | composer/autocomplete.rs:47,443 (confirming) | ✅ | |
| 历史（↑↓、stash 现稿） | prompt/history.tsx:143 | composer/mod.rs:402-472 | ✅ | 首次上翻 stash 活稿语义对齐 |
| 提交（steer/queue 两种 delivery） | prompt/index.tsx:438,453,1092 | store.rs/mini/queue.rs（steer/queue） | ✅ | |
| 粘贴摘要（≥3 行或 >150 字符折叠成芯片） | prompt/index.tsx:1454 | composer/mod.rs:1086-1131 | ✅ | paste_should_summarize 阈值一致 |
| 附件/图片粘贴 | prompt/attachment.ts:96, local-attachment.ts:176 | composer/attachment.rs:488, mod.rs:1200 | ✅ | |
| metadata 行（字符/token、模型） | component/prompt/metadata.tsx:101-146 | composer/render.rs:128-274 | ✅ | promptMetadataLayout 有序丢弃规则对齐 |
| draft stash | prompt/stash.tsx + draft-stash.ts | composer/mod.rs:449-470, DialogKind::Stash | ✅ | |
| move 会话 | prompt/move.tsx:181 | dialog.rs + autocomplete（move_session ctrl+a） | ✅ | |
| 对话框基座（backdrop/esc/ctrl+c 行为） | ui/dialog.tsx:115-156 | dialog.rs:1-196 | ✅ | ctrl+c 清过滤文本后关闭的细节对齐 |
| 通用 select（模糊过滤/预览） | ui/dialog-select.tsx:922 | dialog_select.rs:2019 + fuzzy.rs:805 | ✅ | |
| confirm/alert/prompt/help/导出 | ui/dialog-{confirm,alert,prompt,help,export-*} | DialogKind::{Confirm,Alert,Prompt,Help,SessionExport,ExportResult}（dialog.rs:134-180） | ✅ | |
| 23 个 dialog-* 变体 | component/dialog-*.tsx | dialog.rs DialogKind 枚举（30+ 变体） | ✅ | 全部有对应 Kind；session-list→ServerList、agent→subagent_picker/session.rs |
| toast | ui/toast.tsx:214, theme-error-toast | toast.rs:309 + toast_solid.rs | ✅ | |
| 命令面板（registry/bindings/searchText） | command-palette.tsx:18-25 | command_palette.rs:1-15 | ✅ | searchText 拼接规则、v2 键位表对齐 |
| 会话 tab（溢出/hover marquee/glow/编号点火） | session-tabs.tsx:24-123, tab-pulse.tsx | session_tabs_solid.rs:213-280 + marquee.rs:367 | ✅ | 含 unread glow、上下文菜单（TabMenu） |
| 侧栏会话列表 | routes/session/sidebar.tsx | sidebar.rs:1242 | ✅ | |
| 页脚（prompt/权限/表单/5 个 picker 面板） | mini/footer*.tsx + routes/session/{permission,form}.tsx | footer/{state,solid,permission,form,form_authorization,panel,panels}:2188+945 行 | ✅ | blocker 优先级（permission>form）对齐 |
| devtools | devtools-bar.tsx:611 | devtools.rs:325 | ✅ | |
| 动画引擎（spring/tween） | ui/animation.ts:37-181 | oc-tui-core/animation.rs:1-20 | ⚠️ | 数学"字节级对齐"，但采样从 ref 的 16ms 帧钟降为 100ms 泵 tick（注释自认 G-128 有界偏差）——动画视觉粒度粗于 ref |
| one-cell 微动效 | ui/one-cell-motion.ts:13-111 | mini/spinner.rs（帧表） | 🟡 | 帧剪辑存在（mini/），但 ref 的 BLOCK_* /SEED_* 帧族未在组件层逐一对照 |
| spinner 帧 | spinner-frames.ts:1 | message/rows.rs:34 | ✅ | 10 帧盲文点阵逐字一致 |
| 滚动（page=viewport/2、follow/sticky 再粘） | routes/session/index.tsx:732-805 | oc-tui-core/scroll.rs:1-15 + scroll_accel.rs | ✅ | |
| 滚动加速度 | util/scroll.ts:getScrollAcceleration | scroll_accel.rs（同名） | ✅ | |
| 重连/启动加载 | reconnecting.tsx, startup-loading.tsx | store.rs / home.rs 内嵌 | 🟡 | 无独立组件文件，逻辑并入 store/home（行为存在） |

### 3. 关键差异与风险 Top 5

1. **⚠️ 动画时间粒度**：`animation.rs:7-14` 用 100ms 泵采样替代 ref 16ms `setInterval`（G-128 有意为之）。spring 轨迹按墙钟正确，但帧间跳变约 6 倍粗——shimmer/tab-pulse 等细腻动效可能显"卡顿"，属可感知的行为差异。
2. **🟡 `filetype_for_path` 扩展名表偏窄**（syntax.rs:399-422）：仅 ~20 扩展名；ref 的 `util/filetype.ts` LANGUAGE_EXTENSIONS 覆盖更广（如 `.md`、`.sh`、`.zsh`、`.scala→sc` 等在路径场景下高亮会静默退化为纯文本）。围栏代码块（fence info string）路径不受影响。
3. **语法高亮无 injections**（syntax.rs:70-74）：与 ref 现状一致，但 ref 的 HTML injections 只是"被注释掉"——未来 ref 启用后 Rust 需跟进；vue 文件内模板高亮两版本目前均不完整。
4. **🟡 markdown 渲染器自研**：Rust 未用 opentui 内建 Markdown 而是在 `message/markdown.rs` 重写（1094 行）。对齐靠 golden 测试钉住；ref 内建渲染器更新时（版本升级）Rust 不会自动跟进，是长期漂移面。LaTeX/mermaid 块渲染（:646-758）依赖 unicode-math/ASCII 渲染器，与 ref 的 opentui 内建能力为等效而非同源。
5. **⚠️ 高亮缓存全清策略**（syntax.rs:382-384）：容量满 128 即 `cache.clear()` 全量清空而非 LRU 淘汰——长会话大量代码块滚动时会偶发批量重解析（一次性卡顿），ref（opentui 内建缓存）无此尖峰。

### 4. 缺失文件/功能汇总

**未发现整块缺失**。ref 域内 23 个 dialog-*、ui/ 全部原件、prompt/ 全部 13 件、component/ 主要组件在 Rust 侧均有对应实现（多为合并进 dialog.rs 枚举 / footer 家族 / store，而非同名文件）。次级缺口：
- `filetype_for_path` 扩展名覆盖不全（见风险 2）；
- `reconnecting` / `startup-loading` / `panel-host` 无独立组件文件（功能并入 store/home/chrome，行为等价但难单独审计）；
- `ui/one-cell-motion.ts` 的完整帧族（BLOCK_LOW_COMET 等）仅在 mini/ 出现部分对应；
- `ui/subcell.ts`、`border.ts`、`layout.ts`、`file-path.tsx`、`link.tsx`、`pane-resize*` 未在 components/ 发现同名件（功能散并入 style.rs/layout.rs/wrap.rs 及 dialog 渲染，属布局基建域，非组件层缺口）。

总体结论：该域移植质量异常高——Rust 侧大量以 ref file:line 为规范锚点并有 golden 测试体系；上述风险中仅动画粒度（1）与缓存策略（5）具有真实 bug/体验风险，其余为可接受的工程取舍。
