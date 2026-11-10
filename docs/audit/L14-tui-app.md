# L14 TUI 应用层 — 审查报告

> 审查方式：ref app.tsx(1399) 全文、routes/context/config 全清单；Rust app.rs 1–10492 全文。keybind 默认表逐条对照。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明

**Ref 侧逐文件已读**：`index.tsx`、`app.tsx`(1399 全文)、`runtime.tsx`、`logo.ts`、`audio.ts/.d.ts`、`clipboard.ts`、`editor.ts`、`editor-zed.ts`(结构+关键函数)、`attention.ts`、`model-preference.ts`、`config/index.tsx`(全文 375 行)、`routes/home.tsx`、`routes/session/` 全部 21 文件（重点 form/permission/mount-budget/location-missing/index）、`context/` 全部 29 文件（重点 route.tsx 全文、interactivity、session-retention 全文，其余行数+职责核对并逐一 grep Rust 对应物）、`config/keybind.ts`+`v1/keybind.ts`。
**Rust 侧**：`app.rs` 全文阅读 1–10492 行；`keymap.rs` 全文 + `oc-tui-core/keymap/table.rs` 默认表逐条；`config.rs` 全文；`attention.rs` 全文；`clipboard.rs`/`editor.rs`/`editor_zed.rs`/`editor_ide.rs` 头部+关键实现；`components/command_palette.rs` 命令表/派发段；`store.rs`/`data.rs`/`mini/model_pref.rs`/`sound.rs`/`devtools.rs`/`plugin_page.rs` 定向核对（grep 全 crate 含 mini/ 确认缺失项）。

### 2. 逐项对比表

| 功能点 | Ref 实现 (file:line) | Rust 实现 (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| 启动流程 run() | app.tsx:204-451 | app.rs:7948-8012 | ✅ | 配置预载→终端获取→SSE 连接→输入线程→泵循环 |
| 渲染器配置 (60fps/mouse/alt-screen/kitty) | app.tsx:233-264 | app.rs:8036-8128, 7908-7921 | ✅ | kitty 键盘 push 改为探针应答后推送；?2026 同步更新包为移植新增 |
| TTY/非 TTY 保护 | (CLI 层保证) | app.rs:7954-7958 | 🔵 | 移植版新增显式 is_terminal 检查 |
| CLI→TUI renderer handoff | app.tsx:193-200,218,246-251,437-440 | grep 无 TUI 对应 | 🔴 | terminalHandoff（免拆建闪烁交接）未移植；每次 boot 全新建终端 |
| OPENCODE_DRIVE 模拟前端 | app.tsx:253-256 | grep 无 | 🔴 | 仿真驱动模式未移植（测试基建） |
| OPENCODE_STORY/OPENCODE_ROUTE 初始路由 | app.tsx:336-351 | grep 无 | 🔴 | dev/storybook 初始路由注入未移植 |
| OPENCODE_FAST_BOOT | app.tsx:351,1389 | app.rs:8001-8006,8154-8156 | ✅ | JS Boolean 语义精确复刻 |
| BootArgs (--session/--continue/--prompt/--auto) | app.tsx:364-371,626-685 | app.rs:7775-7804, apply_boot_route:7825-7848 | ✅ | --continue 解析 latest root session 完全一致 |
| --fork | app.tsx:664-685 (TUI 内异步 fork) | oc-cli run/mod.rs:255-283 (CLI 层先 fork) | ⚠️ | 语义等价但时点不同：port 阻塞 boot 前 fork（无占位 dummy 路由闪烁） |
| 缺失 --session id → 新会话命名 | handlers/default.ts | BootArgs.new_session_id + HttpPromptSink | ✅ | |
| Provider 树挂载顺序 (30+ 层) | app.tsx:293-435 | store.rs 信号簇 + run loop 各 init 段 | ✅ | 逐项（Storage/Tabs/Theme/Local/Dialog/History/Stash/Attention/Update/Panel/Plugin）均有对应 init |
| SIGHUP → destroyRenderer | app.tsx:283-287 | terminal_proto.rs install_crash_restore | ✅ | 合并进 crash-restore 守卫（PANIC/SIGTERM/SIGHUP） |
| stderr 重定向到 state 日志 | (bun 运行时行为) | stderr_redirect.rs:8077 | 🔵 | 显式实现，编辑器/挂起时让位 |
| 路由类型 home/session/plugin | context/route.tsx:9-35 | store.rs Route:58-65 + plugin_page.rs | 🟡 | plugin 路由作为独立"页面"状态而非 Route 变体；导航互斥语义已复刻 |
| Home 页 (logo/placeholder/--prompt 门控自动提交) | routes/home.tsx:52-80 | components/home.rs + app.rs:245-281 | ✅ | 提交就绪门逐条复刻 |
| 会话页挂载/SessionFrame | routes/session/index.tsx (3591) | components/session_screen.rs, session.rs | ✅ | |
| location-missing 页 | routes/session/location-missing.tsx | components/location_missing.rs | ✅ | |
| 表单页 (session.form) | routes/session/form.tsx (1175) | components/footer/form.rs + app.rs FormBodyState | ✅ | reply/cancel POST seam 齐全 |
| 权限页 (permission ask) | routes/session/permission.tsx (622) | components/footer/permission.rs + ask_permission | ✅ | --auto 自动应答+失败回滚 |
| dialog-execute/fork/message/timeline | routes/session/dialog-*.tsx | store.rs DialogKind + dialog.rs | ✅ | |
| 全局命令表 (~60 appCommands) | app.tsx:688-1216 | command_palette.rs:130-460 + 派发 2329-3500 | ✅ | 逐命令名核对齐全 |
| 命令默认键位表 | config/keybind.ts | oc-tui-core/keymap/table.rs:503-700 | ✅ | 逐条对照全一致；v1 snake_case 别名兼容 |
| app.exit 空草稿条件层 | app.tsx:1244-1251 | app.rs:2504 + keymap set_command_enabled | ✅ | |
| leader 键/超时 (2000ms) | config/index.tsx | keymap LEADER_TIMEOUT_MS, config.rs:280 | ✅ | |
| keybind 覆盖 + 配置错误 toast | config/index.tsx | keymap.rs + config_error_toast | ✅ | theme 错误 toast 标题逐字复刻 |
| 会话列表/切换/新建/清空/删除 | app.tsx:700-762 | command_palette.rs:2585-2615, store.rs:12866 | ✅ | session.deleted 退 home+toast |
| 标签条 (tabs 全操作) | app.tsx:111-128,784-837 | command_palette.rs:2675-2694, store tabs 簇 | ✅ | |
| 快速切换槽 (quick_switch 1-9) | app.tsx:776-783 | command_palette.rs:2687 | ✅ | |
| tabs 配置 + HERDR_ENV | config/index.tsx:322-329 | config.rs TabsConfig:102-139 | ✅ | 含 legacy enabled 布尔映射 |
| tabs 持久化 (cwd/global 作用域) | session-tabs.tsx:104-115 | app.rs:8351,8432-8454 + storage.rs | ✅ | 恢复不动 boot 路由 |
| @文件附加 | prompt/ + composer | composer/autocomplete.rs (fs.find) | ✅ | 查询守卫（过期结果丢弃）复刻 |
| 图片粘贴/粘贴摘要 | clipboard.ts + prompt/paste | app.rs paste_terminal_text + prompt_paste_compact | ✅ | |
| prompt 组装/提交 (HttpPromptSink) | context + client | app.rs:4748-5160 | ✅ | 失败恢复 apply_send_failure |
| 会话环境变量 PUT | app.tsx:489-499 | app.rs:9036-9059 | ⚠️ | ref 每次导航到会话都会重发；port 每 boot 每 session 仅一次；且用自身进程 env 而非宿主快照 |
| 剪贴板 (读 png/text、写 OSC52/宿主) | clipboard.ts:19-67 | clipboard.rs (Osc52Sink, ClipboardSource) | ✅ | NUL 清除同款；100KB OSC52 上限为移植新增保护 |
| terminal.copy (win32 默认 manual) | app.tsx:546-547 | config.rs:393-399 默认 Select | ⚠️ | port 无 win32→Manual 默认分支（本环境 Linux 无实际影响） |
| 选区复制/按键拦截 (priority 101) | app.tsx:549-571 | app.rs copy_behavior + selection.rs | ✅ | |
| 外部编辑器 (VISUAL/EDITOR) | editor.ts:14-41 | editor.rs:60-130 + run loop 9388-9438 | ✅ | 空格拆分、exit!=0 保留草稿、临时文件清理、附件重对齐全齐 |
| Zed 选区 (sqlite 轮询) | editor-zed.ts | editor_zed.rs + spawn_zed_poll | ✅ | 字节偏移→字符串索引、三态结果复刻 |
| IDE socket (~/.claude/ide/*.lock + env 覆盖) | editor.ts:44-85 | editor_ide.rs | ✅ | 评分/mtime 排序、authToken header |
| attention 引擎 | attention.ts | attention.rs:449-604 | ✅ | 逐函数等价（normalize/strip/clamp/focusSkip/notify 全对照） |
| attention.bell (BEL) | 无（ref 从不发 BEL） | attention.rs:249-252 | 🔵 | 移植新增 opt-in（默认 false） |
| 声音包 (内置 mp3 + 覆盖) | audio.ts + attention-sounds | sound.rs (include_bytes + 系统播放器) | ✅ | 实现途径不同，语义等价 |
| OS 通知 | attention.ts:118 | os_notify.rs OscNotifier | ✅ | OSC 序列直写 |
| model-preference (recent/favorite/variant) | model-preference.ts | mini/model_pref.rs + app.rs:5292 | ✅ | 文档路径 data/tui 一致；脏写回批量落盘 |
| TUI 配置加载/解析/重载 (mtime watch) | config/index.tsx | config.rs + app.rs:10054-10128 | ✅ | 键默认逐项一致 (mouse=true, volume=0.4 等) |
| 主题 (registry/name/mode/system/lock/switch) | context/theme.tsx (420) | theme.rs (1110) + store theme 簇 | ✅ | OSC 10/11 探针 + 焦点时 10s 冷却重探 |
| terminal.title (OSC 0, 40 字截断) | app.tsx:596-622 | app.rs:10304-10314 | ✅ | 变更时才发 |
| terminal.suspend (SIGTSTP/SIGCONT) | app.tsx:1119-1129 | suspend.rs + app.rs:9448-9471 | ✅ | 挂起前后终端拆建/重绘齐全 |
| 鼠标配置热切换 | app.tsx:592-594 | write_mouse_flip app.rs | ✅ | |
| MCP 失败/需认证 toast | app.tsx:519-544 | store.rs:9239 | ✅ | |
| tui.command.execute / tui.toast.show / tui.session.select | app.tsx:1253-1274 | store.rs:12949,12999 + bus dispatch | ✅ | |
| session.moved / session.deleted 打开列表维护 | app.tsx:1276-1292 | store.rs:12866-12890 | ✅ | |
| Reconnecting 覆盖层 (1s/5s 阈值) | app.tsx:1294-1318 | store.rs:12324-12336 + chrome_solid.rs:410 | ✅ | 阈值表一致 |
| 更新通知 | context/update-notification.tsx (网络订阅) | update.rs + notice 文件 mtime watch | 🟡 | port 依赖 CLI 检查器落盘的 notice 文件；TUI 内 `/update` 检查不访问网络 |
| devtools 条 | app.tsx:457 (`debug.devtools ?? channel==="local"`) | devtools.rs（仅 config 门控） | ⚠️ | 缺 local channel 默认开启分支 |
| 插件宿主 (boot/路由/命令/主题/keybind) | plugin/* + app.tsx:396-401 | plugin_boot.rs (1617) + plugin_ui.rs + plugin_page.rs | ✅ | StartupLoading 门控/失败均 ready 复刻 |
| Migration overlay (1s 轮询) | component/migration-overlay | migration_overlay.rs spawn | ✅ | |
| session-retention (evict 保留窗) | context/session-retention.ts | store.rs:831,1672 | ✅ | |
| interactivity 禁用上下文 | context/interactivity.tsx | 无直接对应 | 🟡 | 框架级子树禁用概念未显式移植，低风险 |
| 退出尾声 epilogue | app.tsx:446-450 | farewell_solid.rs + app.rs:10480-10488 | ✅ | |
| 退出原因 stderr | app.tsx:447-448 | cli_error_message app.rs:5044 | ✅ | |
| 崩溃 ErrorBoundary → ErrorComponent | app.tsx:305-310 | catch_unwind + error_mode 粘性崩溃帧 (app.rs:10186-10249) | ✅ | 栈+版本段落齐 |
| boot 输入丢弃窗 | 无 | app.rs BOOT_DISCARD_WINDOW 300ms:7924,8698 | 🔵 | 防"上次退出残键"护盾（有意新增，可能吞掉极快首键） |

### 3. 关键差异与风险 Top 5

1. **terminalHandoff 未移植**（ref app.tsx:193-200/246-251）：CLI 已建 renderer 时免拆建交接缺失，port 每次 boot 全量重置终端（模式突发、清屏），CLI→TUI 切换有闪烁/模式双写风险面。
2. **session.environment 重发语义**（app.rs:9036 vs ref app.tsx:489-499）：ref 每次（重）进入会话路由都会 PUT 环境；port 每 boot 每 session 一次，切走再切回不重发——长时间多标签使用后环境可能过期。
3. **更新通知通道改构**：ref 的 UpdateSource 网络订阅 + TUI 内 `/update` 检查，port 全部依赖 CLI 检查器的 notice 文件；若 CLI 检查器未运行或版本落后，TUI 内"检查更新"给出的是本地陈旧结论。
4. **devtools 默认门控缺 local-channel 分支**（devtools.rs:4 vs app.tsx:457）：本地开发态 devtools 条不自动出现（仅调试体验）。
5. **dev 用 env 启动项缺失**（OPENCODE_STORY / OPENCODE_ROUTE / OPENCODE_DRIVE 全无）+ **boot 300ms 输入丢弃窗**为 port 有意新增——前者影响测试/storybook 工作流，后者与 ref 按键时序不同（极快首键被吞）。

### 4. 缺失文件/功能汇总

**缺失（🔴）**：terminalHandoff 交接（app.tsx:193-200）；OPENCODE_DRIVE 仿真前端 (app.tsx:253)；OPENCODE_STORY/OPENCODE_ROUTE 初始路由注入 (app.tsx:336-351)。
**部分（🟡）**：plugin 路由非 Route 枚举变体（语义近似）；update-notification 网络通道；interactivity 上下文；mount-budget 逐函数对应物（并入 store 窗口化）。
**移植新增（🔵）**：attention.bell 配置；stderr 重定向守卫；boot 丢弃窗/崩溃恢复守卫；OSC52 100KB 上限；TTY 检查。
总体评价：该域移植度极高（命令表、keybind 默认表、attention/config/editor 等均逐条对齐且有 file:line 交叉引用注释），缺口集中在 dev/启动交接/更新通道三条线，无核心交互功能缺失。
