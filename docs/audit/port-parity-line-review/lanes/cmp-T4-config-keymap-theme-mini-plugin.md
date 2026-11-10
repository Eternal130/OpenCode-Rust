# T4 结论: keymap/theme 资产与解析逐字节对齐、TuiConfig 全键可解析但 `session.tps` 默认值反转（ref=true/rust=false）；mini 为忠实压缩重构；插件机制换架构（bun guest）且缺 guest 侧 markdown 渲染器注册面。

## keybind 全集对照（分组完整表）

依据：ref `packages/tui/src/config/keybind.ts:45-301`（Definitions 共 **230** 条，含 leader）vs rust `oc-tui-core/src/keymap/table.rs:507-847`（DEFAULT_TABLE 共 **238** 条）。默认键值逐条比对。

**结论：ref 全部 230 条在 rust 表中均有同名同默认值条目；rust 另有 8 条补充条目（ref 中为组件内 layer 直绑，不在 Definitions）。**

| 分组 | 键（ref≡rust 默认一致） |
|---|---|
| leader | `leader`（ctrl+x ✓） |
| app.* | `app.exit`(ctrl+c,ctrl+d,\<leader\>q)、`app.clear`(ctrl+l)、`app.debug`/`app.console`/`app.scrap`/`app.toggle.animations`/`app.toggle.file_context`/`app.toggle.diffwrap`/`app.toggle.paste_summary`(none) — 全一致 |
| 命令/帮助 | `command.palette.show`(ctrl+p)、`help.show`、`docs.open`、`opencode.settings`、`opencode.status`(\<leader\>s)、`opencode.debug`、`server.pair`、`service.restart`、`location.reload` — 全一致 |
| diff.*（24+5 deprecated） | open、close(escape,q)、down(j,down)、up(k,up)、page.down(pagedown,ctrl+f)、page.up(pageup,ctrl+b)、half_page.down(ctrl+d)、half_page.up(ctrl+u)、first(gg,home)、last(shift+g,end)、next_hunk(])、previous_hunk([)、next_file(n,alt+down)、previous_file(p,alt+up)、toggle_file_tree(b)、single_patch(s)、switch_source(d)、toggle_view(v)、mark_reviewed(m)、help(?,shift+?,shift+/)、toggle/expand/expand_all/collapse/switch_focus(deprecated,none) — 全一致 |
| prompt/theme | `prompt.editor`(\<leader\>e)、`theme.switch`(none)、`theme.switch_mode`(none)、`theme.mode.lock`(none) — 一致 |
| pane/terminal | `pane.focus.left`(\<leader\>left)、`pane.focus.right`(\<leader\>right)、`terminal.select`(\<leader\>down)、`terminal.toggle`(\<leader\>t)、`terminal.close`(\<leader\>up)、`terminal.suspend`(ctrl+z)、`terminal.title.toggle`(none) — 一致 |
| session.* 导航 | `page.up`(pageup,ctrl+alt+b)、`page.down`(pagedown,ctrl+alt+f)、`line.up`(ctrl+alt+y)、`line.down`(ctrl+alt+e)、`half.page.up`(ctrl+alt+u)、`half.page.down`(ctrl+alt+d)、`first`(ctrl+g,home,alt+home)、`last`(ctrl+alt+g,end)、`message.next`/`previous`/`message.user.next`/`previous`(none)、`messages_last_user`(alt+end) — 一致 |
| session.* 管理 | `sidebar.toggle`(\<leader\>b)、`toggle.scrollbar`(none)、`export`(\<leader\>x)、`copy`(none)、`copy.id`(none)、`move`(none)、`new`(\<leader\>n)、`list`(\<leader\>l)、`timeline`(\<leader\>g)、`fork`、`rename`(ctrl+r)、`delete`(ctrl+d)、`share`、`unshare`、`interrupt`(escape)、`background`(ctrl+b)、`compact`(\<leader\>c)、`aside`(none)、`cd`(none)、`queued_prompts`(\<leader\>q)、`toggle.exploration_grouping`(none)、`verbosity.cycle`(none)、`toggle.thinking`(none)、`undo`(\<leader\>u)、`redo`(\<leader\>r)、`pin.toggle`(ctrl+f) — 一致 |
| session.child | `child.first`(down)、`child.next`(right)、`child.previous`(left)、`parent`(up) — 一致 |
| session.quick_switch.1–9 | 9 条全一致（\<leader\>1..9） |
| session.tab.* | `next`(ctrl+tab,alt+down)、`previous`(ctrl+shift+tab,alt+up)、`history.back`(none)、`history.forward`(ctrl+i)、`next_unread`(alt+shift+down)、`previous_unread`(alt+shift+up)、`close`(\<leader\>w)、`reopen`(ctrl+shift+t)、`select.1–10`（\<leader\>N,ctrl+N / 10=\<leader\>0,ctrl+0） — 全一致 |
| queued/stash | `queued_prompt.undo`(ctrl+u)、`queued_prompt.delete`(ctrl+d)、`stash.delete`(ctrl+d) — 一致 |
| model/agent/mcp/provider/variant | `model.dialog.provider`(ctrl+a)、`model.dialog.favorite`(ctrl+f)、`model.list`(\<leader\>m)、`model.cycle_recent`(f2)、`model.cycle_recent_reverse`(shift+f2)、`model.cycle_favorite`/`_reverse`(none)、`mcp.list`(none)、`provider.connect`(none)、`agent.list`(\<leader\>a)、`agent.cycle`(shift+tab)、`agent.cycle.reverse`(none)、`variant.cycle`(ctrl+t)、`variant.list`(none) — 一致 |
| prompt.* | `submit`(none)、`queue`(\<leader\>return)、`editor_context.clear`(none)、`images.view`(\<leader\>i)、`skills`、`stash`、`stash.pop`、`stash.list`(none)、`clear`(ctrl+c)、`paste`(ctrl+v，preventDefault:false 两端均保留)、`history.previous`(up)、`history.next`(down) — 一致 |
| input.*（35 条） | submit(return)、newline(shift+return,ctrl+return,alt+return,ctrl+j)、move.*/select.*、`line.home`(ctrl+a)/`line.end`(ctrl+e)、`select.line.home`(ctrl+shift+a)/`end`、`visual.line.home`(alt+a)/`end`(alt+e)、select.visual.line.home/end、`buffer.home`/`end`(none)、`select.buffer.home`(shift+home)/`end`、`delete.line`(ctrl+shift+d)、`delete.to.line.end`(ctrl+k)、`delete.to.line.start`(ctrl+u)、backspace、delete(ctrl+d,delete,shift+delete)、undo(ctrl+-,super+z)、redo(ctrl+.,super+shift+z)、word.forward(alt+f,alt+right,ctrl+right)、word.backward(alt+b,alt+left,ctrl+left)、select.word.forward/backward、delete.word.forward(alt+d,alt+delete,ctrl+delete)、delete.word.backward(ctrl+w,ctrl+backspace,alt+backspace)、select.all(super+a) — 全一致 |
| composer.* | `subagent.up/down/select/interrupt`(ctrl+d)、`shell.up/down/select/kill`(ctrl+d)、`terminal.up(down,k)/down(down,j)/select(return)` — 一致 |
| dialog.* | select.prev(up,ctrl+p)/next(down,ctrl+n)/page_up/page_down/home/end/submit(return)、prompt.submit(return)、integration.rename(ctrl+r)/delete(ctrl+d)、worktree.generate(tab)、move_session.new(ctrl+a)/move(ctrl+m)/delete(ctrl+d)/refresh(ctrl+r)、mcp.toggle(space)、plugins.error(space)/install(shift+i)/update(ctrl+u)/check(ctrl+r) — 全一致 |
| prompt.autocomplete.* | prev(up,ctrl+p)、next(down,ctrl+n)、hide(escape)、select(return)、complete(tab) — 一致 |
| 杂项 | `open.menu`(ctrl+o)、`messages.copy`(\<leader\>y)、`permission.prompt.fullscreen`(ctrl+f)、`plugins.toggle`(return)、`plugins.list`(none)、`plugins.install`(none) — 一致 |
| which-key.*（11 条） | toggle(ctrl+alt+k)、layout.toggle、pending.toggle、group.previous/next、scroll.up/down、page.up/down、home/end — 全一致（两端均为"已注册无消费者"的死绑定状态，注释与 ref 事实吻合） |

**rust 有 ref 无（8 条，均对应 ref 组件内 layer 直绑）**：`stats.open/close`（ref stats.tsx:151-153）；`app.exit` diff 层重绑 ctrl+d（ref diff-viewer.tsx:726-732）；`dialog.select.section.prev/next`(alt+up/down)（ref dialog-select.tsx:547-562）；`composer.subagent.toggle-activity`(ctrl+a)、`composer.close`(escape,ctrl+c)、`composer.tab.prev/next`(left/right)（ref composer/index.tsx:72-82）。

**语法对齐**：`LEADER_DEFAULT="ctrl+x"`、`LEADER_TIMEOUT_MS=2000`；Chord 解析含 `gg` 连击、`<leader>` 展开、`>` 分段（table.rs:135-242）；override 接受 v1 蛇形名+点状名。差异 D-1/D-2。

## TuiConfig 键对照表

ref `config/index.tsx:69-249` ↔ rust `oc-core/cli_config/mod.rs:295-331`（Info，全键解析）+ `oc-tui/src/config.rs:147-204`（TuiConfig 切片）。

| ref 键 | rust 解析 | 默认值对齐 |
|---|---|---|
| theme.{name,mode} / keybinds / plugins[] / leader.timeout | ✓ | ✓ |
| scroll.{speed,acceleration} | ✓(Info) | 解析但未查到消费（D-6） |
| attention.{notifications,sound,volume,sound_pack,sounds} | ✓ | ✓(false/false/0.4/"opencode.default"/{})；rust 额外 `bell` 键（ref 无） |
| diffs.{source,wrap,tree,single,view} | ✓ | ✓（diff_viewer 消费） |
| terminal.{title,copy} | ✓ | copy ✓；title 消费存疑（D-5/D-6） |
| prompt.{editor,paste,image_preview} | ✓ | paste ✓；editor/image_preview 未查到消费 |
| session.* 9 键 | ✓ | **tps ✗（ref 默认 true，index.tsx:322；rust false，config.rs:234/378/501）→ R-1**；其余 ✓ |
| tabs.{mode,enabled,scope,layout,indicators} | ✓ | ✓（含 HERDR_ENV 门；layout 在 session_tabs_rail.rs 消费） |
| mini.* 10 键 | ✓(Info:225-251) | ✓（mini/runtime.rs 直读） |
| debug.{devtools,timing,turn_tokens} | ✓ | turn_tokens ✓；devtools/timing 组件内有 |
| experimental / animations / mouse / cursor | ✓ | mouse(true ✓)/cursor(block,blinking ✓)；animations/experimental TUI 侧消费未见（D-6） |

## theme 对照表

- **资产**：ref 33 个 v1 json + v2/opencode.json 与 rust **逐文件名一致且 md5 全等**（33 v1 聚合哈希相同，v2 byte-identical）。
- **token schema**：ref schema.ts（HueStep 9 档、SemanticHue 3 必需、ActionState 5 态、syntax 9 token、markdown 14 token、diff 族、@dialog 覆盖、light/dark 双态）≡ rust schema.rs 全实现。
- **color**：**无色彩空间行为差异**——ref 本身即 OKLCH 方案（color.ts:25-77），rust 自注"verbatim port"。清单文档"ref 任意色值 vs rust OKLCH"的说法不成立（已证伪）。
- **system 亮暗探测**：Rec.601 luma >0.5 规则一致；`generateSystem` 完整移植（oc-tui/theme.rs:599 含测试）。
- **select/expand/resolve/v1/v1-migrate/discovery**：均有对应模块；组织差异（ref discovery/system/select 在 tui/src/theme 消费侧，rust 集中 oc-theme）。

## mini 对照表

ref 38 文件 ≈20.6k LOC → rust 17 文件 ≈10.6k LOC（solid 组件树→状态机+自绘 Frame 重构）：

- runtime 编排（firstPaint→ensureSession→modelLoad/catalog→stream→queue 主循环、clientGeneration 中止链、demo、RESIZE_DELAY=250）↔ runtime.rs+lifecycle.rs+queue.rs；verbosity 预设写 `mini.*` 五键对齐（runtime.rs:840-850 ≡ runtime.ts:253-266）。
- stream→scrollback：transport.rs(2800L) 事件 fold 逐函数标注 ref 行号；fragment reconciler（G-68）在 fragment.rs；image 去重；subagent 孪生流（footer_subagent/footer_tree ≡ stream-v2.subagent.ts 状态投影）。replay/replayOnResize/本地行环在 lifecycle/runtime。
- footer 各视图：FooterKey/FooterEffect/PanelState 键驱动机 + golden 测试锁定输出。
- demo.rs(553L ≡ demo.ts 的 /permission /form /fmt 拦截集)、splash、theme（OKLab 量化+故意与主 TUI 分叉的注释性测试）、mono、verbosity、variant（model_pref 偏好持久化）、spinner（work_spinner 14 变体）均有。
- mini 的 footer.prompt（ref 1555L）与 footer.command（1205L）未逐行核到每子特性，以 golden+函数面比对为准。

## 插件机制对照表

| 面 | ref（进程内 solid 插件运行时） | rust（bun guest 子进程） | 判定 |
|---|---|---|---|
| 发现 | config 目录+项目 .opencode/plugins 目录扫描、symlink、merge 去重 | plugin_boot.rs:151-228（OPENCODE_CONFIG→全局 opencode.json(c)→cwd 上行，最近者后置覆盖） | ✓（目录级 plugins/ 自动发现 ref 有、rust 走 config 声明 — D-7） |
| 加载/运行时 | in-process import，bun/node 双支持 | oc-plugin-host bun 子进程 NDJSON JSON-RPC、无 bun 优雅降级 | 架构不同，语义对齐 |
| 热重载 | watch.ts（父目录 watch、rename-safe、symlink 双挂） | host `reload_now` digest 比对 + generation cache-bust | ✓（轮询/摘要 vs fs.watch，D-8） |
| API 面 | api.tsx Context：markdown.registerCodeBlockRenderer、keymap.layer/dispatch/shortcuts、storage、ui.{dialog×4,toast}、format、router、panel、tabs(5 方法)、model.current/variant、slot(5 placement) | host ops：slot.{materialize,click}、tui.{theme,route,layout}、ui.{prompt,confirm,alert}、toast、storage、command.add、theme.register、rpc、event.subscribe；keymap 经 owned 层；slot 五 placement + Slotted/Suppressed/Degraded 降级 | **guest 侧无 markdown 代码块渲染器注册 op（R-2）；ui.tabs/model.variant guest 面未查到对应 op（D-9）** |
| builtins | builtins.ts 13 项 | latex/mermaid 静态编译进 markdown.rs；notifications_plugin；plugins 管理对话；stats_page；diff_viewer；home/footer/sidebar 组件 | ✓（**Storybook 无 rust 对应** D-10，ref 本身仅开发环境注册） |
| PluginRoute 渲染 | render.tsx + `<PluginRoute>` + Missing 兜底 | plugin_page + materialize_open_route/apply_route_content + plugin_ui 投影 | ✓ |
| 键位/命令投影 | registry routes/slots/markdown + palette | plugin_boot 投影 palette 行/group 规则/chord 进 owned keymap 层 | ✓ |

devtools ✓；logo 逐字符一致（`go` 变体在 mini/splash.rs:183）。

## 行为差异清单 D-n

- **D-1 BindingValue 顶层对象不可解析**：ref 允许顶层 KeyStroke 对象或 BindingObject（keybind.ts:27）；rust `parse_value`（table.rs:348-388）仅 string/array/false，顶层对象 → None → 丢弃并 toast。对象形式单键覆盖失效。
- **D-2 修饰键折叠**：rust 将 meta→alt、hyper→super（table.rs:71-72）；ref 为独立字段——理论上可绑定 rust 无法区分的组合（默认表未用到，低危）。
- **D-3 未知 keybind 键**：ref `parse()` 抛错；rust 记录+toast+跳过（防砖注释）——故意的健壮性分叉。
- **D-4 win32 下 terminal.suspend/input.undo 适配缺失**：ref `resolve`（index.tsx:288-296）在 win32 解除 terminal.suspend 并把 ctrl+z 并入 input.undo；rust 恒保留 ctrl+z=suspend。
- **D-5 rust 独有 `attention.bell` 配置键**（默认 false）；ref 无。
- **D-6 解析未消费键**：scroll.{speed,acceleration}、prompt.{editor,image_preview}、animations、experimental、terminal.title——TUI 侧消费点未找到，列为待确认。
- **D-7 插件目录自动发现**：ref 扫描 `<config>/plugins/` 与项目 `.opencode/plugins/` 目录；rust 发现走 config plugins[] 声明——目录约定放置的本地插件若未声明是否被拾取需复核。
- **D-8 热重载机制**：fs.watch vs digest 轮询——触发时延与 rename 场景不同。
- **D-9 guest 插件 ui.tabs/model.variant API 面未见 host op 对应（ref api.tsx:245-297）。
- **D-10 Storybook（OPENCODE_STORY）无 rust 对应**；ref 也仅开发环境注册，可接受。
- **D-11 mini footer 视图架构**：状态机重构，交互等价性靠 golden 锁定，非逐组件同构。
- **D-12 theme 模块归位差异**：纯组织差异。

## 正确性风险 R-n

- **R-1 `session.tps` 默认值反转**：ref `tps: input.session?.tps ?? true`（config/index.tsx:322）→ 默认显示 tok/s；rust `session_tps: false`（config.rs:234/378/501 三处）。默认 UX 差异，建议改 true。
- **R-2 guest 插件无法注册 markdown 代码块渲染器**：ref `ctx.markdown.registerCodeBlockRenderer`（api.tsx:168-178，last-wins）；rust latex/mermaid 为静态内置，oc-plugin-host ops 无对应注册通道——依赖该 API 的第三方 TUI 插件在 rust 下静默失去功能。
- **R-3（低）win32 ctrl+z 语义**：rust 当 suspend 而非 undo，与 ref 相反。
- **R-4（低）D-1/D-2 叠加**：对象形式 override + meta/hyper 修饰的键位被丢弃且仅 toast，用户配置静默回退默认键。

## 覆盖声明

- **逐行读完**：ref config/keybind.ts（359L 全）、config/index.tsx（375L 全）、theme/src/tui/{schema,color,select}.ts、tui/src/theme/system.ts、plugin/{api.tsx,builtins,discovery,watch(头部)}、mini/runtime.ts（1125L 全）；rust oc-tui-core keymap/table.rs（922L 全）、oc-tui config.rs（822L 含测试）、oc-theme {builtins,system}、mini/{mod,lifecycle(函数面),transport(函数面),queue(函数面),demo,theme}、plugin_boot.rs（函数面+头注）、plugin_ui.rs、notifications_plugin.rs（头注）、components/{plugin_slots,devtools,home(logo)}。
- **以工具核对**：33+1 主题资产 md5/字节比对；keybind 两表逐条人工 diff（230/238 全量，无抽样）；cli_config Info 键全集 grep；latex/mermaid 注册面 grep（发现 R-2）。
- **未逐行**：mini footer.prompt/footer.command 子特性、stream-v2.transport.rs 全函数体、plugin watch.ts 剩余 70 行、feature-plugins home/prompt/sidebar 组件体、expand/resolve/syntax/v1(-migrate) 字段级比对（以结构面+LOC 量级+行号引用密度替代）。
- 不信任 .md 清单：已证伪两处（"36 内置主题"实为 33+1；"rust OKLCH vs ref 任意色值"实为 ref 本身即 OKLCH）。
