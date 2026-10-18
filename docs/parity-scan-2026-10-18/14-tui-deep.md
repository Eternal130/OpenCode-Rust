# TUI 补漏深扫（parity-scan 2026-10-18 · 14-tui-deep）

- 第二轮「补漏」通道：只针对 `00-MASTER.md` §6 与 `06-tui.md` §16 明确声明**未深扫**的区域做行为级 diff。已登记项（06 差异表 #1–#34、MASTER 各 lane）不重复展开，仅在需要补充证据/细化时引用。
- 参考侧重点文件（全读）：`packages/tui/src/routes/session/index.tsx`（3591 行）、`routes/session/{anchors,mount-budget,history,message-navigation,location-missing}.ts(x)`、`routes/session/permission.tsx`（622 行）、`routes/session/form.tsx`（1175 行）、`component/dialog-experiments.tsx`、`context/session-retention.ts`、`config/new-session-location.ts`、`component/session-tabs-rail.tsx`、`context/interactivity.tsx`、`context/update-notification.tsx`、`component/plugin-route-missing.tsx`。
- Rust 侧重点文件：`crates/oc-tui/src/store.rs`（9748 行，消息摄取/命令/权限表单队列/滚动）、`components/session*.rs`、`components/command_palette.rs`（dispatcher）、`components/footer/{permission,form,form_authorization,mod}.rs`、`components/dialog.rs`、`oc-tui-core/src/keymap/table.rs`、`oc-tui-core/src/scroll.rs`、`data.rs`、`app.rs`、`config.rs`、`oc-schema/src/{misc_events,permission}.rs`。
- 方法：静态只读；双侧 `path:line` 均为本次实读定位。无法静态确认的交互标 `UNVERIFIED`。未跑 build/test/双端 live。
- 编号沿用本通道前缀 **TD-**，避免与 06 的 #N 冲突。

---

## 1. 会话路由行为（`routes/session/index.tsx` ↔ `store.rs` / `components/session*.rs`）

### TD-01 · S1 · WRONG · NEW —— 六类 wire 消息被 TUI 双摄取路径静默丢弃（含活动摘要行）

- **EXPECTED**：`SessionMessageView` 渲染全部消息联合（`routes/session/index.tsx:1782-1812`）：
  - `shell` 消息 → `ShellMessage`（`$ cmd` 块 + killed/timeout/exit-code 错误文案，`index.tsx:2166-2183`）；
  - `agent-switched` / `model-switched` / `location-switched` → `SessionSwitchMessageV2`（"Switched agent …"、`switchLabel`、"↳ Moved to …"，`index.tsx:1904-1932`）；
  - `system`/`synthetic` → `SessionNoticeMessageV2`：**活动摘要行** `↳ Subagent finished · <desc>` / `! Subagent failed`，可点击跳转子会话（`onMouseUp → navigate`，`index.tsx:1978-1982`），error/cancelled 配色（1960-1963）。
- **ACTUAL**：
  - SSE 路径 `message_from_event` 只处理 `user`/`assistant`/`skill`，其余 `_ => None`（`crates/oc-tui/src/store.rs:7673-7783`，丢弃臂在 7782）；
  - 快照路径 `message_from_info` 只处理 User/Assistant/Idle/Compaction/Skill，`_ => None`（`crates/oc-tui/src/data.rs:548-680`）；
  - 转录模型 `Message` 枚举无对应变体（`crates/oc-tui/src/components/message/model.rs:199-277`；`Synthetic` 变体只建模 `instruction_paths`，无 subagent/shell 完成通知、不可点击）。
  - wire 侧类型存在：`crates/oc-schema/src/session_message.rs` 有 AgentSwitched/ModelSwitched/LocationSwitched/Shell/Synthetic；服务端也在产（`oc-core/src/session/message_updater.rs:96` 产 `agent-switched`）。
- **用户可见影响**：shell 回合块、切换 agent/model/位置的系统行、后台子代理/shell 完成摘要（含点击进入子会话）全部不出现。子代理完成在转录里只剩工具行，无「↳ finished」可点击摘要。
- 备注：`!` shell 模式本身缺失是 06 #14（KNOWN）；本条是**消息类型渲染面**的独立缺口——即使 shell 消息由其它客户端产生，本端也不显示。

### TD-02 · S1 · MISSING · NEW —— 消息导航命令族 5 条注册即死 + MessageActions「Jump to」不跳转

- **EXPECTED**：`session.message.next/previous`、`session.message.user.next/previous`（`index.tsx:1099-1126`，经 `scrollToMessage`/`findMessageBoundary` 步进，`routes/session/message-navigation.ts:18-48`）；`session.messages_last_user`（alt+end，`index.tsx:1079-1098`）；`jumpToMessage`/`alignMessage` 带导航 slack 占位（`index.tsx:681-730`、`NAVIGATION_SLACK_ID` 1419-1421）。
- **ACTUAL**：键表已注册全部 5 条（`crates/oc-tui-core/src/keymap/table.rs:680-684`），dispatcher 无任何对应臂（`crates/oc-tui/src/components/command_palette.rs:1621-2321` 全表核对，落在 `_ => Dispatch::Unknown`）；无锚点注册表（无 `anchors.ts` 对应物）。连带：MessageActions 对话框的 "Jump to" 只清对话框不滚动（`components/dialog.rs:869-912` 的 `_ => {}` 臂 + `command_palette.rs:1193`）。
- **用户可见影响**：alt+end 与 4 条导航命令按键无反应；消息动作里的「Jump to」是空操作。

### TD-03 · S1 · MISSING · NEW —— `session.parent`（子会话内 `up` 返回父会话）无实现

- **EXPECTED**：绑 `up`、`enabled: !!session()?.parentID`（`routes/session/index.tsx:1272-1288` + `config/keybind.ts:139`）——子会话（子代理视图）里按上键回父会话。
- **ACTUAL**：键表注册 `session_parent => "up"`（`oc-tui-core/src/keymap/table.rs:636`），dispatcher 无臂；子会话路由由 picker 接管键（`app.rs:1265-1273`），其 `stroke_in_picker` 无 parent 导航。
- **用户可见影响**：进入子代理会话后只能靠 tab/escape 退出，`up` 习惯路径断裂。

### TD-04 · S1 · MISSING · NEW —— 重连/会话失效恢复链缺失（sync、权限/表单重取、not-found 收尾）

- **EXPECTED**（`routes/session/index.tsx:313-353`）：连接建立后 `session.sync({children:true})` + `permission.sync` + `form.sync`（含后代会话，313-320）；会话不存在 → toast `Session not found: <id>` + 关 tab/回 home（334-341）；sync 出错 → 错误 toast + 关 tab（344-352）。
- **ACTUAL**：resync 只做 `session.list` + 每 open tab 的 `message.list`（`crates/oc-tui/src/data.rs:273-312`）；无 permission/form 重取（`permission.asked`/`form.created` 是纯 SSE 队列，`store.rs:6695-6717`）——断线期间发生的询问永久丢失；transcript fetch 失败仅 `log_skip`（`app.rs:5225-5229`），无 toast、不关 tab。
- **用户可见影响**：网络闪断后挂起的权限/表单不再出现（会话看起来卡死）；已被删除会话的 tab 永久停留空白。
- 备注：06 #30 的 SessionLocationMissing 屏（UNVERIFIED/PARTIAL）是另一条恢复路径，本通道复核 ref 实现（`location-missing.tsx:14-81`，SessionQuestion + DialogWorkspaces + `session.move`）确认 Rust 无对应物，维持 06 结论。

### TD-05 · S1 · PARTIAL · NEW —— 挂载预算与历史分页缺失（全量加载、无 tail/backfill 窗口）

- **EXPECTED**：`TRANSCRIPT_TAIL_ROWS=40` / `TRANSCRIPT_BACKFILL_CHUNK=60`，尾优先挂载、靠近顶部时按预算回填（`routes/session/index.tsx:130-135, 394-487`；`mount-budget.ts:10-38` 权重按折叠组计）；滚顶渐进 `loadMore` + 锚点补偿（`history.ts:1-61`）；`session.first` 走「全量 loadMore(all) + beforePublish commit + Loading 指示 + 可中止」（`index.tsx:808-876`、指示行 1425-1427）。
- **ACTUAL**：一次 `message.list` 全量拉取（`limit: None`，`crates/oc-tui/src/data.rs:322-337`）；无 cursor/loadMore API 面；渲染侧每帧测量全部行（`components/message/list.rs:721-789` 仅有 paint 窗口，无挂载预算）；`session.first` 只是 `scrollTo(0)`（`oc-tui-core/src/scroll.rs:100-103` + `store.rs:4207-4214`）。
- **已 PORTED 部分**：新开 tab 的转录拉取有「Loading session history…」状态（`app.rs:5213-5217` 的 `set_history_loading`）。
- **用户可见影响**：长会话（数千行）首开拉全量、每帧行测量成本线性增长（性能影响 UNVERIFIED，需运行时确认）；无渐进式回填体验。
- 备注：v2 服务端 `message.list` 有 cursor 分页（ref client `loadMore`），本条只登记 TUI 行为差异。

### TD-06 · S2 · PARTIAL · NEW · UNVERIFIED —— 滚动恢复无内容锚点

- **EXPECTED**：离底时按「已挂载条目锚点」保存（part/group 目标 + screenY，`index.tsx:518-547`；`anchors.ts:37-50` 的 `containsAnchor`），回挂/切 tab 时按锚点恢复并按需回填（`restoreScrollPosition`，`index.tsx:548-595`），导航时用 slack 占位维持视口（`message-navigation.ts:8-16`）。
- **ACTUAL**：每会话仅 `FollowState::{Sticky, Free{offset,maximum}}` 绝对偏移（`crates/oc-tui/src/store.rs:4185-4223`；`oc-tui-core/src/scroll.rs:38-82`），跨 tab 切换靠 map 持久化。无内容锚定：锚点上方插入内容不会保持原行。
- **用户可见影响**：切回旧 tab 大致恢复位置，但流式/后台消息插入上方时视口内容会漂移。精确 UX 差需 live 复核（UNVERIFIED）。

### TD-07 · S1 · PARTIAL · NEW —— 队列管理器缺 undo；steer/delete 为本地重发语义；pending steer 无区分表面

- **EXPECTED**（`routes/session/index.tsx:627-675, 1445-1447, 2201-2249`）：
  - QueuedPrompts 对话框三动作：enter=steer、ctrl+d=delete、**undo=取消所选行并把内容（含 files/projectedPromptInput）放回 composer**（652-671，含 shell 模式守卫 toast）；
  - 全部经 `session.inbox.update/cancel`（`mutatePending`，608-626，single-flight + 失败 toast）；
  - pending（steer 中的）用户消息边框用 `border.base` 而非 agent 色（2214），点击弹「Move to queue / Delete」选择框（2228-2241）。
- **ACTUAL**：对话框只有 steer+delete（`components/dialog.rs:241-268`）；steer 实现为「删本地队列行 + 重新 POST prompt」（`dialog.rs:847-863` 的 `forward_submission`），从不调用 `session.inbox.update`；全局 `queued_prompt.undo` 只本地删最后一行、不回填 composer、不调 `inbox.cancel`（`store.rs:3767-3776`）；`Message::User` 无 delivery 字段（`components/message/model.rs:200-207`），pending steer 无边框区分、无点击对话框。
- **用户可见影响**：误入队的 prompt 无法「撤回编辑」（文本丢失）；多客户端/重连下队列状态不同步；steer 中的消息与已确认消息视觉无差。

### TD-08 · S2 · MISSING · NEW —— AssistantRetry 倒计时行缺失

- **EXPECTED**：`⚠ Retrying in Ns · attempt N · <error>` 实时倒计时（`index.tsx:2344-2369`），由 `AssistantFooter` 在 error 行位置渲染（1875-1881）。
- **ACTUAL**：`Message::Assistant` 无 `retry` 字段（`components/message/model.rs:208-233`；`store.rs:7757-7773`、`data.rs:596-609` 摄取均不读 retry），`rows.rs:686` 仅注释提及。
- **用户可见影响**：限流/断连重试期间转录无任何指示（服务端确实在重试，用户只看到停顿）。

### TD-09 · S2 · MISSING · NEW —— BackgroundToolHint（后台化提示）缺失

- **EXPECTED**：运行中的 shell/subagent 超过 3s 后在转录尾渲染 "Press <ctrl+b> to move running work to the background"（`index.tsx:130, 1748-1780`，`createDelayedPresence` 延迟出场）。
- **ACTUAL**：无对应物（grep `background_tool_hint`/文案无命中）。
- **用户可见影响**：`ctrl+b` 后台化能力（已移植，`command_palette.rs:1888-1891`）可发现性显著下降。

### TD-10 · S2 · MISSING · NEW —— TurnTokenUsage（`debug.turn_tokens`）缺失

- **EXPECTED**：每回合折叠行 `Tokens: N steps · latest: X new · Y cached · Z total` + 缓存击穿警告 + 展开逐 step 表（`index.tsx:1588-1746`）。
- **ACTUAL**：无对应物（grep `turn_tokens` 无命中；`TuiConfig` 不读 `debug.*`）。
- **用户可见影响**：调试/成本观察面缺失（dev 向）。

### TD-11 · S2 · MISSING · NEW —— `session.toggle.thinking` / `session.toggle.scrollbar` 注册即死；TUI config 不读 `session.*` 行为键

- **EXPECTED**：两命令切换 `session.thinking`（hide/show）与 `session.scrollbar` 并落 config（`index.tsx:1018-1049`）。
- **ACTUAL**：键表注册（`oc-tui-core/src/keymap/table.rs:582, 688`），dispatcher 无臂（也不在 2160-2190 的吞键清单里，落 Unknown）；`TuiConfig` 只读 theme/leader/attention/image_preview/tabs/sidebar/mouse/cursor（`crates/oc-tui/src/config.rs:133-161`），`session.thinking/scrollbar/tps/markdown/grouping` 均未进 TUI 配置（分组 toggle 是 store 本地态 `store.rs:6273`，不读也不写 config）。
- **用户可见影响**：思考块一旦 hide 无法再展开（也无命令入口）；滚动条不可开；`session.tps` 开关无效（tok/s 恒显示）。
- 备注：`session.verbosity.cycle` 已在 06 #33 登记（KNOWN），不重复。

### TD-12 · S2 · MISSING · NEW —— `/rename <title>` 带参直达缺失

- **EXPECTED**：slash 带参数：非空 → 直接 `session.update` 设标题；空 → `title.generate`（`index.tsx:902-911`）。
- **ACTUAL**：dispatcher 的 `session.rename` 忽略 `args`，恒开改名对话框（`command_palette.rs:1727-1750`）；slash 表有 `rename`（`composer/autocomplete.rs` V2_SLASH）但 accept 后走同一无参路径。
- **用户可见影响**：`/rename 修复登录` 这类一步改名习惯需改走对话框两步。

### TD-13 · S2 · MISSING · NEW —— `session.copy`（转录复制 / `/copy`）缺失

- **EXPECTED**：`session.export` GET → `formatSessionTranscript` → 剪贴板 + success toast（`index.tsx:1180-1200`；格式化器 3503-3533）。
- **ACTUAL**：dispatcher 只有 `session.copy.id`（`command_palette.rs:2091-2109`）；无 `session.copy` 臂、无 `/copy` slash。
- **用户可见影响**：整段转录一键复制缺失（Export 对话框可绕行）。

### TD-14 · S2 · MISSING · NEW —— QueuedPromptDock 与 "Jump to latest ↓" 不可点击

- **EXPECTED**：dock 行 `onMouseUp → openQueuedPrompts`（`index.tsx:2312-2342`）；跳底槽 `onMouseUp → toBottom`（1428-1442）。
- **ACTUAL**：dock 有渲染（`components/session.rs:289-295, 982+`）、跳底槽有渲染（`components/message/list.rs:765-782`），但 release 处理器的命中清单（`app.rs:738-806`）无二者矩形。
- **用户可见影响**：两处指示只能看不能点（键盘路径均在）。

### TD-15 · S2 · PARTIAL · NEW · UNVERIFIED —— `--prompt` 自动提交不等模型就绪

- **EXPECTED**：等 `local.model.ready && catalogReady && agent/model current` 后才自动 submit（`index.tsx:597-604`）。
- **ACTUAL**：boot 段直接 `submit_prompt`（`app.rs:4729-4735`），无就绪门。
- **用户可见影响**：理论上服务器模型解析未就绪时首条 prompt 可能失败（服务端默认模型兜底存在，实际影响 UNVERIFIED）。

### §1 已核对为对齐/PORTED 的项（不另立条）

六滚动命令 + first/last 语义（`scroll.rs` 注释即引 `index.tsx:763-805`）；`session.compact/undo/redo/export/background/share(unavailable toast)/fork/timeline` dispatcher 全在（`command_palette.rs:1866-1891, 2051-2060, 1822-1847`；undo 含「Nothing to undo」+ composer 回填 `store.rs:4609-4647`，与 ref 974-999 对齐——仅缺 pending-delivery 分支与 files 投影，属 TD-07 同族小项）；`messages.copy`/`copy.id`；`<leader>q` 队列管理器 + 空回车 steer 队首（`store.rs:2099-2119` 注释引 `index.tsx:1504-1508`）；revert banner 全量（计数/文件 ±明细/redo 键提示/点击 clear，`rows.rs:916-954` ↔ `index.tsx:2084-2164`）；compaction 行（usage `N in · M out`、Provider compaction、cancelled，`rows.rs:729-760`）；assistant footer 宽度门 28/36、interrupted、error 行（`rows.rs:640-696` ↔ `index.tsx:1875-1899`）；子代理工具行（continue ↳/model 徽标/Background 徽标/点击导航，`rows.rs:121-184` + `app.rs:796-798`）；tab 未读/`attention: Permission`（`session.rs:76-97`，question 态缺——并入 TD-16 影响）；图像条 config 门 + 前 3 张 + `+N more`（`store.rs:983-986`、`rows.rs:371+1492`）；窄屏 sidebar 关闭层（`app.rs:1724-1747` ↔ `index.tsx:242-246`）；`--auto` 权限自动应答（`app.rs:1524-1543` ↔ `index.tsx:254-270`，仅逐条×drain 节奏差）；autoaccept 元数据行（`session.rs:1206-1208`）。

---

## 2. Permission / Form 展示与交互深度

### TD-16 · S1（建议按 S0 处理）· MISSING · NEW —— 主 TUI 的表单阻塞面整体不存在（不可见、不可答、不拦键）

- **EXPECTED**：`FormPrompt` 挂在会话路由 composer 位（`routes/session/index.tsx:1476-1483`），权限/表单挂起时 `disabled`（226）替换掉 Prompt——表单在场时**无法**输入 prompt；表单可答（1175 行交互体）。
- **ACTUAL**：
  - `form.created` 只入全局 footer 队列（`store.rs:6708-6717` → `footer/mod.rs:92-99` 置 `FooterView::Form`）；
  - 渲染：`frame()` 只挂 Permission 与 picker 面板（`app.rs:2087-2137`），`FooterView::Form` 无任何 mount（grep 全 crate，`render_form_body`/`render_form_authorization` 仅 `mini/lifecycle.rs:373-379` 与 golden 测试调用）；
  - 键路由：`press()` 分支 Dialog > Diff > **Permission** > ErrorToast > Picker > route（`app.rs:1245-1276`），无 Form 分支——表单挂起时按键全部落进 composer，可继续发 prompt；
  - HTTP 侧亦无 `form.reply/cancel` 提交路径（`app.rs` 无 form reply 调用）。
- **用户可见影响**：MCP elicitation / 插件表单触发时：会话服务端阻塞等待应答，TUI 端**看不到任何询问**，composer 照常可用，回合「假死」。这是与 06 S0-1（插件 UI slot）同量级的整面缺失，因触发频率低于权限询问而列 S1。
- 关联：tab 的 `attention: "question"` 态（`context/session-tabs.tsx:164-168`）也随之缺失（`session.rs:95-96` 只有 Permission）。

### TD-17 · S1 · WRONG · NEW —— `boolean/number/integer` 字段类型使整张表单反序列化失败而被丢弃

- **EXPECTED**：字段类型全集 string/number/integer/boolean/multiselect/external + `when` 条件（`routes/session/form.tsx:97-117`；校验 `util/form.ts:72-110`）。
- **ACTUAL**：`FieldKind` 只有 Text/Select/Multiselect/External（`components/footer/form.rs:29-50`）；`form.created` 用 `if let Ok = serde_json::from_value::<FormRequest>`（`store.rs:6712-6716`）——含 boolean/number/integer 字段的表单**整包反序列化失败且静默丢弃**。服务端确实会产这些类型（MCP elicitation 的 JSON-Schema→表单映射 `oc-plugin-host`/`oc-core elicitation.rs:283-474`；08 lane §3.10 确认 schema 校验全集）。
- **用户可见影响**：与 TD-16 叠加：这类表单连队列都不进，表现为模型调用挂起无任何提示。

### TD-18 · S1 · WRONG · NEW —— 权限交互体用 mini footer 三段机，非会话路由两段机

- **EXPECTED**（`routes/session/permission.tsx`）：
  - 两阶段 `permission | reject`（20）；选项 once/always/reject，**选中 always 时正文区即时预览 always-lines**（223-231），enter 直接 `reply("always")`（243-247）——无二次确认；
  - **reject 自由文本阶段仅子会话（parentID）出现**（248-252）；根会话 reject/escape 立即回 `reject`；
  - `escapeKey: "reject"`：权限阶段 esc = 直接拒绝（432-438, 493）；
  - reject 段内 ctrl+c 清空已输入文本（284-298）；全屏切换 `permission.prompt.fullscreen` + Portal 展开（454-463, 514-523）。
- **ACTUAL**（`components/footer/permission.rs`，mini 模型）：
  - 三阶段 Permission/Always/Reject：`always` 需再按一次 enter 确认（`permission_options` 296-306、`permission_run` 361-390）；
  - `permission_escape` 从权限阶段**永远进入 reject 文本段**（424-437），根会话上 esc 不再立即拒绝；
  - reject 文本段对所有会话开放（462-481）；无 ctrl+c 清空；无全屏切换（mount 高度固定 `PERMISSION_MOUNT_HEIGHT`，`app.rs:2093`）。
- **用户可见影响**：esc 语义反转（v2 一键拒绝、rust 变输入框）；always 多一次确认击键；权限面板不可放大看长 diff（叠加 TD-19）。
- 已 PORTED 部分（不重复登记）：once/always/reject 选项按 `save` 模式门控（`permission.rs:311-319` ↔ `permission.tsx:232-240`）、左右/tab 移动、鼠标 pill 选择+释放即答（`app.rs:1384-1476`）、pointer 悬停跟选、△ 图标与标题、`permissionAlwaysLines` 文案（`permission.rs:231-239`）。

### TD-19 · S1 · MISSING · NEW —— edit 权限的 diff 预览体缺失

- **EXPECTED**：`EditBody`：`diff` → 可滚动 PatchDiff（split/unified 按宽 120 自适应、语法高亮、行号、全套 diff token）；否则 `patch` 文本按 diff 语法着色；再否则 "No diff provided"（`permission.tsx:22-111`）。外部目录权限显示 Patterns 列表（171-179）。
- **ACTUAL**：`permission_presentation` 的 `edit` 臂只给 `Edit {file}` 标题、`lines` 恒空（`components/footer/permission.rs:99-102`；模块头注释自认 "diff/patch rendering stays with the diff lane"）。external_directory 的资源行有（192-207）✓。
- **用户可见影响**：文件修改授权时看不到改什么就要做决定——权限安全审查能力减半。

### TD-20 · S2 · PARTIAL · NEW —— 权限展示行丢失工具输入（wire 无 input，未做 source 回查）

- **EXPECTED**：`permissionPresentation` 的 input/metadata 取自**工具 part**（经 `request.source` 的 messageID/partID 回查 assistant 消息，`permission.tsx:122-132`），据此渲染 `$ {command}`、`◉ {description}`、agent 名等。
- **ACTUAL**：wire `permission.asked` 的 `Request` 本就不带 input（`crates/oc-schema/src/permission.rs:44-58`，与 ref schema 一致），Rust TUI 直接从事件反序列化（`store.rs:6695-6701`），`PermissionRequest.input` 恒空 → `permission_presentation` 的 `value("command")/("agent")/("description")` 全部落空（`permission.rs:85-148`）：shell 显示 `- <resource>` 而非 `$ cmd`；subagent 显示 "General Subagent" 无描述；webfetch 靠 metadata.url 尚可（153-157）。
- **用户可见影响**：询问正文信息量下降（命令仍以资源列表形式可见，格式与内容完整度不同）。

### TD-21 · S2 · PARTIAL · NEW —— 表单深度子集：校验/条件/单字段直答/tab 条/review/草稿缺失

- **EXPECTED**（`routes/session/form.tsx` + `util/form.ts`）：`when` 条件显隐（97-110）；单字段（boolean/带 options 的 string）直答（111-117）；窄屏 "Field N of M · X/Y completed"、宽屏 tab 条 + Submit（783-847）；review 页逐字段核对 + external 确认态（1063-1123）；跨重挂载草稿（drafts map，54-57, 211-228）；完整校验（minLength/maxLength/pattern/email/uri/date/date-time/number min-max/integer/minItems/maxItems/非 custom 选项闭集，`util/form.ts:72-110`）；external 字段 open/copy/I finished 流（477-511）。
- **ACTUAL**（`components/footer/form.rs`，mini 模型）：无 `when`；`single` 只认 Select（212-217）；校验仅 required（119-139）；无 tab 条/review 页/草稿持久化；external 仅有渲染件 `form_authorization.rs`（且只在 golden 中使用，见 TD-16）。数字键 1-9 直选、custom 行、multiselect toggle 等 mini 语义在。
- **用户可见影响**：在 TD-16 修复后才会显现为「能用但浅」；条件字段全量铺开、复杂校验不拦截。

---

## 3. dialog-experiments.tsx 与实验配置面

### TD-22 · S2 · MISSING · KNOWN（06 §2 表已列，补证据与行为判定）

- **EXPECTED**：`DialogExperiments`（`component/dialog-experiments.tsx:18-84`）：DialogSelect 列出实验、footer on/off、←/→ 切换写 `config.experimental.<id>`；入口在 DevToolsBar 的 "Experiments" 项（`devtools-bar.tsx:429-437`）。**v2.0.24 的 `experiments` 数组为空**（`dialog-experiments.tsx:16`），空态显示 "No experiments available"。
- **ACTUAL**：Rust TUI 无 DialogExperiments、无 DevToolsBar（06 #20 KNOWN）；`experimental` 配置 map 在 `oc-core/src/cli_config/mod.rs:325` 已解析但无 TUI 消费者。
- **用户可见影响**：**当前为零行为差**（上游实验列表为空）；待上游新增实验项时该面整体缺失。维持 S2/KNOWN，升级条件=上游实验列表非空。

---

## 4. session-retention / new-session-location 的 TUI 表现（不重复 13 号通道的配置分析）

### TD-23 · S2 · PARTIAL(EXTRA 倾向) · NEW —— 无客户端会话数据逐出（内存无界）

- **EXPECTED**：`createSessionRetention`（`context/session-retention.ts:4-41`）：保留 = 打开 tabs ∪ 最近浏览 3 个 root ∪ 当前；其余 root 从 client cache 逐出（新后代也逐出），重访问时重新拉取。挂载点 `context/session-tabs.tsx:139-145`（`limit: 3`）。
- **ACTUAL**：`store.messages` 的 `by_session` map 只增不减（`store.rs:3518-3530`，无 evict 调用点）；重开 tab 会 refetch 一次（`app.rs:5203-5217` 的 `fetched_transcripts` 集合）。
- **用户可见影响**：基本无（重开有刷新，无陈旧显示）；差异是客户端内存随浏览会话数无界增长、后台 tab 不释放。属资源管理而非功能缺失。

### TD-24 · S2 · MISSING · NEW —— `session.new_location: "inherit"` 不生效（新会话恒用启动目录）

- **EXPECTED**：`newSessionLocation(mode, launch, current, unavailable)`（`config/new-session-location.ts:3-16`）：inherit 且当前会话位置可用 → 新会话继承当前位置；否则回启动目录。调用点：`session.new`/`session.clear`（`app.tsx:715-757`）、tab `+`（`context/session-tabs.tsx:406-418`）、mini（`mini/runtime.ts:940`）。
- **ACTUAL**：`TuiConfig` 不读 `session.new_location`（`config.rs:133-161`）；home 提交建会话时用 sink 构造时捕获的启动目录（`app.rs:2912-2978`，`SessionCreateBody{location: directory}` 固定值）；无 unavailable 守卫。
- **用户可见影响**：在 worktree/子目录会话里按 `+` 或 `/new`（配置 inherit 时）得到启动目录的新会话而非当前目录。默认值 "launch" 下两端一致，故 S2。

---

## 5. 其余指定表面的实际用户行为

### TD-25 · S1 · MISSING · KNOWN 子项（06 #3 的组成部分，补充细节）—— 垂直 rail 的搜索控件与半行边框

- **EXPECTED**：垂直 tab 布局底部 rail 有 `⌕` 搜索钮（点击 → `tabs.search` 或 `session.list`，`component/session-tabs-rail.tsx:20-53`）；rail 顶/底用 `▄/▀` 半行块与相邻面板拼接（`SessionTabHalfRow`，71-94；below-highlighted 状态行 54-66）。
- **ACTUAL**：垂直布局整体缺失（06 #3：仅水平 strip，`components/session_screen.rs:270`）；rail 控件随之缺失（grep 无 `session-tabs-rail` 对应物）。
- **用户可见影响**：并入 06 #3 的「宽屏布局选择缺失」；本条补充：即使补垂直布局，还需带上 rail 搜索入口与半行接缝渲染。

### TD-26 · INFO · 无独立差异 —— `context/interactivity.tsx`

- ref：`InteractivityProvider enabled={activePane()==="session" && !fullscreen()}`（`component/session-frame.tsx:287`）在终端面板/全屏面板激活时剥夺 prompt 与权限输入焦点；RejectPrompt/FormPrompt 用它门控 textarea 焦点（`permission.tsx:279,356`、`form.tsx:65`）。
- rust：无该 context，但唯一能使其为 false 的两个表面（TerminalPane、全屏 PanelHost）本身缺失（06 #14/#15）→ 当前无可观察差异。待补面板时需一并引入焦点门控。不计数。

### TD-27 · S2 · PARTIAL · NEW —— 更新横幅不可交互；remote+installed 未隐藏

- **EXPECTED**（`routes/home.tsx:125-174`）：横幅点击 → remote ? dismiss : installed ? exit : 打开 DialogUpdate("notification")；hover 高亮；`!remote || available` 显示门（remote 的 installed 不显示）。通知去重/持久化（`context/update-notification.tsx:41-62`，versions 末 100 条、client 不挤 server）、open 预置状态（96-115）。
- **ACTUAL**：去重/持久化/client-server 优先级/通知文件监视齐（`store.rs:3422-3447`、`app.rs:4597-4613, 5484-5522`，dismissed 持久化往返 5182-5189）；横幅文案含 remote/installed 形态（`components/update.rs:61-78`、`home.rs:414-419`）。但：横幅无 hit rect、无点击行为（`app.rs` release 清单 587-806 无 banner）；remote+installed 仍渲染（无 `!remote || available` 门）。
- **用户可见影响**：看到更新提示后只能 `/update` 手动打开对话框；远端「已安装」通知在 v2 会隐藏而 rust 仍显示一行。

### TD-28 · S2 · MISSING · KNOWN 子项（06 #22）—— PluginRouteMissing 兜底屏

- **EXPECTED**：`OPENCODE_ROUTE` 指向不存在的插件路由时渲染 "Unknown plugin route: {id}/{name}" + "go home" 按钮（`component/plugin-route-missing.tsx:3-21`）。
- **ACTUAL**：`Route` 仅 Home/Session（`store.rs:44-49`），无插件路由自然无兜底屏（grep 无对应物）。
- **用户可见影响**：当前不可达（启动注入本身缺失，06 #22）；补路由面时需带上该兜底。维持 KNOWN 子项。

---

## 6. 计数与汇总

| 维度 | 计数 |
|---|---:|
| 本通道登记项 | **28**（TD-01…TD-28，TD-26 为 INFO 不计入差异）→ 有效差异 **27** |
| 严重度 | S0 候选 **1**（TD-16，建议按 S0 处理）；S1 **9**（TD-01/02/03/04/05/07/17/18/19，另 TD-25 为 KNOWN 子项不计新 S1）；S2 **16**；INFO 1 |
| 分类 | MISSING 14 · WRONG 4（TD-01/17/18 及 TD-24 记 MISSING）· PARTIAL 8 · EXTRA/PARTIAL 1（TD-23）|
| KNOWN/NEW | **NEW 23**（TD-01…TD-21、TD-23、TD-24、TD-27）；**KNOWN/子项确认 4**（TD-22、TD-25、TD-26、TD-28）|
| UNVERIFIED | TD-05（性能）、TD-06（滚动恢复精确 UX）、TD-15（--prompt 时序后果）——需 live 复核 |

修复优先级建议（本通道内）：**TD-16+TD-17+TD-18+TD-19 一个簇**（表单挂载面 + 权限交互模型归位会话路由版，都是 `footer/permission|form` 换体会话路由实现）；**TD-01+TD-04 一个簇**（消息类型补全 + 重连 resync 扩展，同在 `store.rs` 摄取层）；然后 TD-02/03（dispatcher 补臂）、TD-07（inbox API 化）。

## 7. 覆盖清单

**已逐行为核对（双侧实读）**：
- ref：`routes/session/index.tsx` 全 3591 行；`anchors.ts`/`mount-budget.ts`/`history.ts`/`message-navigation.ts`/`location-missing.tsx` 全读；`permission.tsx` 全 622 行；`form.tsx` 全 1175 行；`dialog-experiments.tsx`、`session-retention.ts`、`new-session-location.ts`、`session-tabs-rail.tsx`、`interactivity.tsx`、`update-notification.tsx`、`plugin-route-missing.tsx` 全读；`dialog-message.tsx`、`home.tsx`（更新横幅段）、`app.tsx`（session.new/clear 段）、`context/session-tabs.tsx`（retention/attention/add 段）、`util/form.ts`（校验段）、`config/keybind.ts`（相关绑定）、`devtools-bar.tsx`（experiments 入口段）、`schema/session-message.ts`（类型联合）。
- rust：`store.rs` 消息摄取/命令/权限表单队列/滚动/队列/revert/update 段；`command_palette.rs` dispatcher 全表（1621-2321）与 palette props；`components/session*.rs` 转录/侧栏/tabs 装配；`footer/{mod,permission,form,form_authorization}.rs` 全读；`dialog.rs` 键处理与提交臂；`message/{model,rows,list,tool_detail}.rs` 行族；`oc-tui-core/{keymap/table.rs 相关行, scroll.rs}`；`data.rs` resync/fetch；`app.rs` press/frame/mouse/boot/update 段；`config.rs` TuiConfig；`oc-schema/{misc_events.rs 权限事件, permission.rs Request}`；mini form 渲染唯一调用点。
- 交叉核对 `06-tui.md` §14 差异表避免重复；`08-plugin-platform.md` §3.10 确认表单服务端 OK（TUI 面为本通道新发现）。

**确认对齐/PORTED（本通道复核项）**：六滚动命令+first/last、compact/undo/redo/export/background/share、fork/timeline、messages.copy/copy.id、队列 dock 渲染 + 空回车 steer + `<leader>q`、revert banner 全量、compaction 行全量、assistant footer（宽度门/tps/interrupted/error）、子代理行族、图像条（门+3 张+more）、窄屏 sidebar 关闭层、`--auto` 自动应答、权限 pill 鼠标交互、更新通知去重/持久化/优先级、`--prompt` 种子提交（存在性）。

**仍未覆盖（留给后续/live）**：双端运行时差分（滚动恢复、挂载性能、`--prompt` 时序、busy-tab 指示——06 §6 已列）；主题 token 消费面逐组件扫描（06 §16 ③，本通道未展开）；mini footer 宽度截断数值级比对；`prompt.queue` 与服务端 inbox 的完整对账（依赖 TD-07 的 API 化后再核）。
