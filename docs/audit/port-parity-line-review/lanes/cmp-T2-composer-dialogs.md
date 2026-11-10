# T2 结论: T2 域（prompt 管线 + composer 四 tab + dialog 族）整体为高保真逐行移植，行为对齐度约 95%；发现 1 个确定性死键（Sessions 对话框 pin/unpin 无分发路径）、若干有记录的 bounded deviation 与 2 处附件标签语义偏移，其余差异均为无害实现细节。

## 对比表

### prompt 管线（ref packages/tui/src/prompt/ ↔ rust oc-tui/src/components/composer/）

| 项 | ref | rust | 对齐 |
|---|---|---|---|
| parseFileLineRange（`#N`/`#N-`/`#N-N`、`#5-3` 折叠、非法后缀保留全文） | parse.ts:1-16 | autocomplete.rs:1573-1612 | ✅ 语义逐条对齐 |
| parseSlashHead | parse.ts:18-26 | autocomplete.rs(directory_trigger :691-708) | ✅（`/cd` 独占参数补全） |
| EditablePromptInput 投影 | codec.ts:11-29 | composer/mod.rs PromptDraft/HistoryEntry::to_json :260-277 | ✅ 结构等价（uri/data 重组装 AttachmentPayload :49-67） |
| normalizePromptContent（单行尾 \n/\r\n 剥离） | content.ts:1-13 | editor.rs:52 | ✅ |
| promptOffsetWidth/displaySlice/mention·slashTrigger | display.ts:5-61 | autocomplete.rs:564-639（G-144） | ✅ grapheme+换行计 1 完全一致 |
| @mention 重对齐（最长文本优先、protected ranges、min-cost max-matching DP） | mention.ts:20-144 | mention_realign.rs:46-181 | ⚠️ DP/分组/保护区间逐行对齐；代价用 char offset 替代 display width（自记录 bounded deviation） |
| 粘贴占位符展开（back-to-front、replace 占位/范围两式） | part.ts:3-15 | mod.rs expand_pasted_ranges :187-202 / expand_pasted_placeholders :1164 | ✅ |
| 图片附件去重/标签（`[Image N]`/`[PDF N]` 编号、payload 复用） | attachment.ts:40-96 | attachment.rs:298-324 | ⚠️ 见 D-3/D-4 |
| 本地粘贴路径→附件（HTTP(S) 跳过、多路径、字节预算） | local-attachment.ts | attachment.rs:260-291 | ✅（MAX_LOCAL_ATTACHMENT_BYTES 预算逐项扣减） |
| history 环（MAX 50、dedup vs 最新、jsonl append/trim 改写、自愈重写） | history.tsx:60-143 | mod.rs:365-488 + storage.rs:233-263 | ✅ 上限/持久化策略一致；⚠️ 导航守卫差异见 D-8 |
| PastedText 追踪（source range + 展开平移） | history.tsx:11-58, mention.ts:76-97 | PastedPart :126-133 + expand；editor 往返走 realign_draft :192-251 | ✅（编辑器回程 `pasted: []` 对齐 index.tsx:570-573） |
| frecency（MAX 1000、`freq/(1+Δdays)`、86400000、按 lastOpen 截断、resolve(cwd)） | frecency.tsx:10-79 | autocomplete.rs:710-870 + storage.rs:241-287 | ✅ 公式/上限/淘汰全对齐 |
| directory-completion（`~`/`..`/绝对路径前缀、exact-value、隐藏点目录、大小写不敏感前缀） | directory-completion.ts:7-98 | autocomplete.rs(directory_trigger/recents 族 :684-870) | ✅（54-90 的每分支未逐行复核） |
| directory-recents（per-project、cap 10、touch/remove） | directory-recents.ts:12-36 | autocomplete.rs RecentDirectory :712-717 + store | ✅（ctrl+d 两击 armed：mod.rs destructive_requests :523-527） |
| stash（MAX 50、push/pop/remove、jsonl 改写） | stash.tsx:14-93 | store.rs:10526-10825 + storage.rs:293-304 | ✅ 含两击删除 armed 状态（dialog.rs:740-747） |
| traits（PromptMode normal/shell、capture 集） | traits.ts:16-29 | mod.rs ComposerMode :140-150 + shell_mode_layer :1562 | ✅ `!`@caret0 进入、esc/backspace-0 退出、Enter 走 session.shell |
| 粘贴折叠阈值（≥3 行或 >150 字符） | index.tsx:1452-1465 | mod.rs:137-138 + paste_should_summarize :1088 | ✅ 常量一致 |

### composer 四 tab（ref routes/session/composer/ ↔ rust store.rs PickerState + subagent_picker.rs + picker_solid.rs）

| tab | ref | rust | 对齐 |
|---|---|---|---|
| 骨架（tabs 注册顺序 Subagents/Shell/Terminals、left/right 切换、esc/ctrl+c 关闭、单 tab 退化、hints 行） | index.tsx:21-157 | subagent_picker.rs:96-170,280-351；app.rs sync_composer_tab_layers :3211 | ✅（`tabs ←/→` 恒绘——端口最小 2 tab 恒真，无害） |
| Subagents（sessionFamily 列表、active/inactive ctrl+a 过滤、current 高亮、`{prefix}{agent}: {title}`、Running 右缘、up@0 关闭、down 取模环绕、select 导航、interrupt 仅 running） | subagents-tab.tsx:39-194 | store.rs:11846-11990 + subagent_picker.rs:172-278,334-345；app.rs:4506-4510 | ✅ 含 5 行窗口居中滚动近似 |
| Shell（仅 running、首行命令、select 开 DialogShellOutput、kill=shell.remove） | shell-tab.tsx:20-111 | store.rs:6226-6260 + subagent_picker.rs Shell 分支；app.rs:4511-4514 | ✅ kill 走 DELETE，output 对话框 G-6 对齐 |
| Terminals（entries+`+ New terminal` 槽、含 new 槽取模、terminals 能力门控注册） | terminals-tab.tsx:24-66 | store.rs:6177+,11951+ + set_terminal_available 门控 | ✅ |
| 渲染生效路径 | — | picker_solid.rs 消费 picker_panel_paint（M6 单 solver） | ✅ 双实现已收敛（手绘 painter 已删） |

### 对话框（ref component/dialog-* ↔ rust DialogKind + dialog.rs/dialog_solid.rs/command_palette.rs）

| dialog | ref | rust | 对齐 |
|---|---|---|---|
| agent | dialog-agent.tsx:10-30 | ServerList{agent.list}（command_palette.rs:879） | ✅ |
| model（Favorites/Recent 分区、排序 opencode-go>opencode→provider→Free→release、fuzzy 优先 favorites、deprecated 过滤、favorite ctrl+f、variant 接力、Connect an integration） | dialog-model.tsx:29-216 | command_palette.rs model_list_props（:818+）+ dialog.rs:507-516 favorite | ✅ 分区/排序/门控逐条对齐 |
| variant（Default + 非 default 变体、model 选择后接力） | dialog-variant.tsx:10-39 | DialogKind::Variant（app.rs:3524） | ✅ |
| config 设置项 | dialog-config.tsx（~30 行设置） | dialog_config.rs:1-518（含 Volume/Window title/Copy behavior/Developer tools、win32 copy 默认、live 消费者+写穿） | ✅ 行集合未逐行全 diff |
| experiments | dialog-experiments.tsx | DialogKind::Experiments（v2.0.24 空表） | ✅ |
| debug | dialog-debug.tsx | DialogKind::Debug + copy 动作 | ✅ |
| error-details（esc 回、c 复制、i investigate、滚动） | dialog-error-details.tsx | DialogKind::ErrorDetails{…6 字段}（dialog.rs:1009+） | ✅ |
| status | dialog-status.tsx | DialogKind::Status（MCP 清单） | ✅ |
| update（Skip + Update/Restart、Checking/Failed 态） | dialog-update.tsx:49-96 | UpdateDialogState（app.rs:10078+） | ✅ |
| image-preview（xlarge 居中、←/→ 循环） | dialog-image-preview.tsx:34-35 | dialog.rs:626-630 + ImagePreviewItem | ✅ |
| integration（列表→账号→方法→Key/OAuthCode/Form/Pending 全状态机、账号 delete 两击、rename） | dialog-integration.tsx（1045L） | DialogKind::Integration{,Accounts,Method,Key,Code,Form,Pending} + integration_account_delete | ✅ 状态机拆为独立 Kind |
| mcp（状态行 + toggle + 错误详情） | dialog-mcp.tsx | DialogKind::Mcp / McpError | ✅ |
| skill | dialog-skill.tsx | DialogKind::Skill（submit 插 `@skill`） | ✅ |
| open.menu（Sessions recents + Projects、→ worktrees 视图、ctrl+n 新建） | dialog-open.tsx | dialog_open.rs open_menu_props/open_worktrees_props | ✅ |
| pair | dialog-pair.tsx | dialog_pair.rs | ✅ |
| session-list（pin ctrl+a/ctrl+f、delete 两击、当前会话 ●、失败 toast） | dialog-session-list.tsx:240-280 | ServerList{session.list} + delete 两击（dialog.rs:504-529）+ pin action 声明（command_palette.rs:910-914） | ⚠️ pin 无分发（R-1） |
| session-rename | dialog-session-rename.tsx | DialogKind::SessionRename | ✅ |
| shell-output | dialog-shell-output.tsx | DialogKind::ShellOutput{shell_id}（G-6）+ ToolOutput shell 角色 | ✅ |
| stash | dialog-stash.tsx | DialogKind::Stash + 两击 delete | ✅ |
| theme-list | dialog-theme-list.tsx | DialogKind::ThemeList | ✅ |
| workspaces / file-changes / worktree-name | dialog-workspace*.tsx, dialog-worktree-name.tsx | dialog_worktrees.rs（move/new/delete 两击/refresh、vcs.status 文件行、generate_slug tab 生成、enter 确认 slugify） | ✅ |
| command-palette（ctrl+p：活 keymap 命令 + Suggested 分区 + settings 行、fuzzy） | command-palette.tsx:18-64 | command_palette.rs 静态 PALETTE_COMMANDS（~470 行）+ suggested 预置 | ⚠️ 静态表 vs 动态注册（D-5） |
| footer/permission（三段 ask、Allow once/Always/Reject、Reject 自由文本、fullscreen） | session/permission.tsx | footer/permission.rs + form_authorization.rs | ✅（mini footer 共享件） |

## 行为差异清单 D-n

- **D-1 mention 重对齐坐标空间**：ref 用 display 宽度做 cost 锚（mention.ts:42,105）；rust 用 char 索引（mention_realign.rs:10-18,91,111）。宽/窄 grapheme 只扰动 tie-break 代价，不改匹配集——自记录 bounded deviation。
- **D-2 无文本 mention 处理**：ref 对 `!mention.text` 原样保留；rust 丢弃空 chip（mention_realign.rs:224-228）。out-of-band 无 chip 附件经 $EDITOR 往返后丢失。
- **D-3 附件标签复用判据**：ref 复用要求 uri + [name,description] 元数据 + mention.text 全等（attachment.ts:81-87）；rust 仅比对 mime+data（attachment.rs:308-313）。同 payload 不同 name/description 的重复粘贴：ref 给新标签，rust 复用旧标签。
- **D-4 非图像/PDF 内联附件**：ref `attachmentKind` 对其它 data: URI 抛错（attachment.ts:14-18,78）；rust 把一切非 image mime 标成 `[PDF N]`（attachment.rs:306）。`data:text/plain` 得到错误的 PDF 标签而非报错。
- **D-5 命令面板来源**：ref 从活 keymap 注册表取命令（含插件注册命令）；rust 为静态 PALETTE_COMMANDS 表。插件运行时新增命令不会出现在面板。
- **D-6 pinned 持久层**：ref `local.session.togglePin` 落本地持久存储；rust `Store::toggle_pin` 只写内存 tabs 状态（store.rs:6929-6936），未见持久化 drain。
- **D-7 picker 滚动近似**：ref ScrollBox 按需滚动+选中居中；rust 固定 5 行窗口居中 clamp。列表 ≤5 行时视觉等价。
- **D-8 历史导航守卫**：ref `move()` 在当前输入与条目文本不一致且非空时拒绝移动（history.tsx:110）；rust `PromptHistory::previous/next` 不设此守卫——编辑过的草稿上按 ↑ 仍走历史（草稿经 staging 保留，round-trip 等价，但"拒绝导航"语义丢失）。
- **D-9 `tabs ←/→` hint 恒绘**：ref 仅 tabList>1 时绘制；rust 端口自认最小 2 tab 恒真。无害。

## 正确性风险 R-n

- **R-1（高）Sessions 对话框 pin/unpin 死键**：keymap 定义 `session.pin.toggle` = ctrl+f（oc-tui-core keymap/table.rs:649）；SelectAction 已声明并渲染 footer（command_palette.rs:910-914）；但 dialog.rs 的 session.list 分支只分发 `session.delete`（dialog.rs:524-529），app.rs/command_palette 分发器均无 `session.pin.toggle`/`toggle_pin` 调用——`Store::toggle_pin`（store.rs:6929）唯一调用方是测试（store.rs:14443-14445）。用户按 ctrl+f 无任何效果。
- **R-2（中，连带 D-6）**：即便补上分发，pinned 存于内存 `tabs.pinned`（store.rs:782），重启即失；ref 持久。且 `hidden: sessionTabs.enabled()` 门控在 rust 侧对应 tabs_enabled 时返回空 pinned（dialog_open.rs:588-594）——门控有，持久无。
- **R-3（低）ctrl+f 双绑定并存**：`session.pin.toggle` 与 `model.dialog.favorite` 同绑 ctrl+f——与 ref 默认一致则无冲突（两对话框互斥），但若 R-1 修复时误在全局层分发会互相干扰。
- **R-4（低）D-3/D-4 的附件标签偏移**会造成 mention 文本与 ref 不一致，进而影响展示去重的编号序列（长会话多次粘贴同名不同述图片时编号漂移）。
- **R-5（低，待验证）**：session-list 删除失败的 toast + 列表内联剔除（dialog-session-list.tsx:262-275）在 rust 侧的错误路径未验证（成功路径两击删除已确认）。

## 覆盖声明

- **逐行读完（双侧）**：prompt/ 全部 13 文件；composer 四 tab 全部 5 文件；mention_realign.rs、attachment.rs 标签/解析段、autocomplete.rs 触发/解析/frecency 段、composer/mod.rs 状态与历史环（1-710 行 + 函数清单核对 674-1598）、subagent_picker.rs 全文、dialog.rs 会话/模型/集成/stash/worktree 键处理段、dialog_config.rs 行表与 change 逻辑段、command_palette.rs model/session props 与命令表结构、store.rs stash/picker/toggle_pin 段、ref dialog-agent/model/variant/session-list/command-palette/dialog-config 设置清单。
- **结构核对 + 抽查（未逐行）**：dialog_solid.rs（DialogKind 消费面枚举核对）、dialog_open.rs、dialog_pair.rs、dialog_worktrees.rs、dialog_select.rs、footer/*（permission 状态机、panels、canvas、solid 仅头注释+关键 grep）、knight.rs（头 30 行：spinner.ts 40ms/blocks 参数对齐）、composer/render.rs 与 autocomplete.rs 中段（@file/fs.find/slash 注册表以测试断言佐证）、ref dialog-integration/mcp/status/update/image-preview/experiments/debug/error-details/shell-output/stash/theme-list/workspaces/worktree-name（读关键行为行，非全文）。
- **未覆盖**：dialog_config.rs 与 ref 的 ~30 设置行逐行值域比对（抽检 8 行一致）；mini/（归 T4）；ui/dialog.tsx 本体（归 T1）；ref prompt/autocomplete.tsx 953 行与 rust autocomplete.rs 1958 行的完整选项集 diff（触发器/解析/最近目录/技能表已核，逐项排序细节未全 diff）；golden 帧正确性未重放。
