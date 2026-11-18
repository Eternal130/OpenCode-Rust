# 沙箱覆盖台账（Coverage Ledger）

> 依据：开发规范 2.2#13（2026-11-18 用户裁定二）——能测的都测，不能有任何已知的遗漏；台账要多审几轮保证无缺漏。
> 状态词汇：**covered**=场景在册；**GAP**=已知遗漏（入排期，两波内不补须波计划显式说明）；**GAP※**=有 ref 锚但无场景（"有锚"=ref 实现存在，可建）；**untestable**=不可测+理由；**audit**=待盘点。
> 审计轮次：R1=A 路（ref 源码穷举）+B 路（事件/配置/状态空间）（2026-11-18）；R2=合并；R3=对抗性攻击（CLI 入口/上游回流/组合矩阵/rust 独有/数据形状/抽查——发现族 I 整缺+维度缺+6 新面+3 锚订正）；R4=对抗性攻击（待）；此后每两波至少一轮对抗审计。
> 可测域维度（R3 新增）：**差分**=双端对拍可测；**单端**=仅 rust 侧可测（ref 枚举不可见：mini 双实现/oc-pty 守护/迁移器自身 UI/--fork --replay 旗标）——H 族及标注「单端」的行走单元 parity 域。

## A. 输入事件层（PTY 真字节）

| 面 | 状态 | 锚/备注 |
|---|---|---|
| 字母/数字/符号/Enter/多行 | covered | 各场景 |
| 修饰键/chords（ctrl+x m 等） | covered | S3/S6 |
| 键位 12 组 ~230 条（diff 键组/input 编辑键组/leader chords） | 部分 | keybind.ts:48-275——常用组 covered，全组矩阵 GAP※ |
| keymap mode/layer 栈（menu push/pop） | GAP※ | context/keymap.tsx |
| 自定义 keybinds/leader 重定义 | GAP | tui config（B） |
| ESC 链（中断/退对话框） | covered | P3 |
| 鼠标左键点击 | covered | mouse_* 族+S 系列 |
| 鼠标滚轮（SGR 64/65） | covered | pty_w11_mouse（W11） |
| 拖选+高亮+copy-on-select | covered | pty_w11_mouse（W11） |
| 拖选范围语义（按字符 vs 含空白） | GAP※ | util/selection.ts（真机⑧残余） |
| 鼠标中键（关 tab） | GAP※ | session-tabs.tsx:894 |
| 鼠标右键（tab 菜单/diff 文件菜单） | GAP※ | session-tabs.tsx:902, diff-viewer-file-menu.tsx:70 |
| 双击（pane 边界重置；词选在 renderer） | GAP※ | pane-resize.ts:52-56 |
| 悬停 hover（tabs marquee/rail/handle/toast action） | GAP※ | marquee.ts, session-tabs.tsx:1652 |
| 粘贴：普通+括号 | 部分 | decodePasteBytes；PTY 真字节缺 |
| 粘贴：图像→附件 | GAP※ | clipboard.ts:25-40, prompt/index.tsx:470-495 |
| 粘贴：大文本折叠（paste_summary） | GAP※ | prompt/index.tsx paste 队列/pasteStyleId ~334-403（R3 订正锚漂移；图像 dedupe/预览 394-403） |
| resize（SIGWINCH） | covered | pty_{chrome,composer,dialog,diff,footer,overlay}_resize.rs 六文件族；**ref 差分面 audit** |
| resize 档位边界（sidebar≤120 折叠/home footer 64/80） | GAP※ | app.tsx:459, home/footer.tsx:7-10 |
| focus/blur（attention FocusState 门控） | audit | attention.ts:23 |
| interactivity 子树禁用 | GAP※ | context/interactivity.tsx |

## B. 会话流与消息生命周期（真实链路）

| 面 | 状态 | 锚/备注 |
|---|---|---|
| boot/连接/settle | covered | t9_1/S1 |
| 慢启动 boot 窗 | covered | t9_8 |
| 发消息→回复+标题 | covered | S2/t9_3 |
| 工具环（bash/edit/read） | covered | tl_tool_loop |
| 工具族其余（write/glob/grep/task+genericToolSummary/InlineToolRow/patch-diff/dialog-execute 全形状） | GAP※ | A：工具形状族有全套 ref 锚 |
| 权限对话框（full：once/always/reject+理由输入；**mini 三阶段**） | GAP※ | permission.tsx:224-244, mini/permission.shared.ts |
| 权限配置分流（ruleset 预授权 vs 运行时询问） | GAP | config permissions（B） |
| 中断（流中 ESC） | covered | P3 |
| 慢 TTFT/慢 TPS | covered | P1/P2 |
| 长会话多轮 | covered | P4-C |
| 会话切换/多标签运行中切换 | 部分 | S4 恢复；运行中切换缺 |
| **多会话并行流式**（多 tab 同时 execution×切换） | GAP | B |
| queued prompts（流中入队/undo/delete/队列管理） | GAP※ | session/index.tsx:631-663, inbox 4 类事件 |
| inbox 事件族（enqueued/delivered/cancelled/delivery.changed） | GAP※ | mini/stream-v2.transport.ts:1040-1072（R3 订正：主前端处理散在 routes/session/index.tsx/render-context.tsx/rows.ts） |
| 会话生命周期操作（fork/rename/move/delete/viewed/pin） | GAP※ | 各事件（B） |
| revert/undo/redo | GAP※ | session.revert.* + undo/redo 键（B） |
| compaction 全周期（started/delta/ended/failed/interrupted/unavailable 分型） | GAP※ | 事件族（B） |
| provider 错误 **12 子类型分型**（rate-limit/quota/auth/timeout/…各不同 UI） | GAP※ | B：现只测单形（G-438） |
| 重试调度（session.retry.scheduled 自动重试态） | GAP※ | B |
| 错误中途部分消息（半截气泡+interrupted/failed 收尾组合） | GAP | B |
| abort vs timeout vs quota 分型显示 | GAP | B |
| 用量/成本（session.usage.recorded→footer/成本行） | GAP※ | B |
| skill 激活（session.skill.activated 渲染） | GAP※ | B |
| shell 会话内终端（session.shell.*/terminal-pane 持久 PTY） | GAP※ | A/B 双报 |
| btw 侧问（session.aside 无工具环生成） | GAP※ | feature-plugins/prompt/btw.tsx |
| subagent 子会话（嵌套会话/mini_subagent 面） | 部分 | mini_subagent_footer 单测；链路级 audit |
| 空态（零会话/零 provider/零消息渲染） | GAP | B |
| 断连恢复 | 部分 | vt_temporal 单元级；真实链路 audit |
| instructions.updated/AGENTS.md 热更 | GAP※ | 事件面（B） |
| worktree.updated | GAP※ | app.tsx（B） |

| transcript 行稳定性（历史 prepend 不跳动） | GAP※ | 上游 40679546d4（R3-2） |
| open 菜单 awaiting-input 置顶排序 | GAP※ | 上游 a2a543c868（R3-2） |
| 文件路径后 read ranges 显示 | GAP※ | 上游 375ff827aa（R3-2） |
| 持久 PTY handoff 不阻断 server 重启/首段输出不丢 | GAP※ | 上游 5ad0786536/334f7a3510（R3-2） |
| message part 数据形状子表（11 tag×metadata 变体） | audit | session-message.ts:46-238（R3-5：需展开为子表，现「工具族其余」一行不足承载） |

## C. UI 组件与对话框

| 面 | 状态 | 锚/备注 |
|---|---|---|
| composer @ 引用+截断 | covered | G-448；PTY 双端 audit |
| composer `/`+参数补全（directory 类型） | audit | autocomplete.tsx:783-803 |
| composer `#` 文件行范围 | GAP※ | prompt/parse.ts:2-13 |
| directory-completion/recents/frecency | GAP※ | A |
| composer 历史 navigation | GAP※ | prompt/history.tsx |
| 图像附件预览（cell 预览+翻图 dialog） | GAP※ | prompt/index.tsx:394-404, dialog-image-preview |
| composer tab 组（shell/subagents/terminals 左右切） | GAP※ | composer/*-tab.tsx（A 新族×3） |
| palette 模型/命令 | covered | S3/S6 |
| command-palette（ctrl+p 命令面板本体） | audit | A（与模型 palette 分列） |
| 侧栏各宽度档 | 部分 | S5(160)/P4b(120)；80/320 缺 |
| **插件 sections（omo-slim 等）** | GAP※ | **触发态存在**：home footer MCP/插件/版本 3 slot 宽度门控 64/80（A 订正）；会话态真机确认 |
| tabs：拖排序/marquee/右键菜单/中键关/未读 | GAP※ | session-tabs.tsx:894-910,1652 |
| tabs 竖排 rail+hover 控件 | GAP※ | session-tabs-rail.tsx |
| TabContextMenu（重命名/复制ID/最近关闭） | GAP※ | session-tabs.tsx:372-420 |
| tabs busy 呼吸灯 | covered | busy_pulse_frames |
| 动效文本（title-shimmer/fade-in/one-cell-spinner 帧族） | GAP※ | A |
| 对话框基元 7 种（alert/confirm/prompt/select/help/export-options/export-result） | audit | ui/dialog-*.tsx |
| 专属对话框族：session-list/open/agent/skill/mcp/theme/config/experiments/status/debug/error-details/variant/session-rename/shell-output/image-preview | GAP※ | A 全列 |
| stash 对话框 | GAP※ | dialog-stash.tsx |
| workspaces/worktree-name/file-changes（VCS） | GAP※ | A 新族 |
| integration（账户管理）/pair（设备配对） | GAP※ | A 新族 |
| update 对话框（check/install/restart 状态机） | GAP※ | A 新族 |
| timeline/fork/message/execute 会话内嵌对话框 | GAP※ | routes/session/dialog-*.tsx |
| FormPrompt（MCP elicitation 表单 UI，form 3 类事件） | GAP※ | A/B 新族 |
| toast（info/error/action hover+tui.toast.show 远程） | 部分 | copy toast（W11）；其余 GAP※ |
| patch-diff 渲染 | 部分 | G-441 edit 行；独立 viewer 面 audit |
| error-component 错误边界 | GAP※ | A |
| theme-error-toast | GAP※ | A |

| plugin select dialog（搜索+actions） | GAP※ | 上游 8380c864bf（R3-2） |
| panel-host/panel 布局 | audit | A |
| migration-overlay（迁移进行中 UI） | GAP※ | A（E 族 UI 面） |
| reconnecting 遮罩（managed/非托管文案分叉） | GAP※ | A+B；G-439 concern① 遗留 |
| logo/masked-text 等杂项 | audit | A |

## D. 配置与热载

| 面 | 状态 | 锚/备注 |
|---|---|---|
| 配置热载→palette | covered | S6/G-437 |
| 插件装载 | covered | T9-7；**热载 GAP※**（plugin.updated 事件） |
| 模型目录（自定义 provider） | covered | S3/G-433 |
| agent/command 定义变更 | GAP | — |
| 主题切换+theme.mode.lock | GAP※ | dialog-theme-list（B+A） |
| animations/mouse/cursor.style/blinking 开关+toggle | GAP | B |
| attention 提醒+音效 5 种+桌面通知 | GAP※ | B+A 新族 |
| diffs wrap/scroll 配置+toggle | GAP | B |
| autoupdate（disable/notify/auto→更新提示） | GAP※ | B+A |
| share（manual/auto/disabled+share/unshare 命令） | GAP※ | B |
| snapshots 开关 | GAP | B |
| default_agent | GAP | B |
| media 图片尺寸（auto_resize/max_*） | GAP | B |
| tool_output 渲染配置 | GAP | B |
| username 显示 | GAP | B |
| experimental.policies | GAP | B |

| setup 期间插件 keymap 生效 | GAP※ | 上游 247eb5cc9a（R3-2，A×D 组合） |

## E. 生命周期与持久化

| 面 | 状态 | 锚/备注 |
|---|---|---|
| 会话存储/重启恢复 | covered | S4/t9_4 |
| v2 数据迁移（凭据/会话/tabs/prefs；migration-overlay UI） | GAP※ | **G-445 根因族沙箱零覆盖**；A：有专用 UI 面可测 |
| 首启 onboarding/无凭据认证引导流 | GAP※ | B+A |
| 升级通知/安装生命周期 | GAP※ | update-notification/context（B+A） |
| 多 channel 数据隔离 | audit | — |

## F. 终端能力

| 面 | 状态 | 锚/备注 |
|---|---|---|
| 宽度档 120/160 | covered | 通道 8 三档+S5/P4b |
| kitty 键盘协议 | 部分 | **锚订正（A）：解析在 @opentui/core renderer 层**；端到端 audit |
| 剪贴板写（host+terminal 双通道：OSC52 为 terminal 通道之一） | GAP※ | app.tsx:55-61 clipboard.writeText（R3 订正：双通道及各自失败面） |
| 剪贴板图像读取 | GAP※ | clipboard.ts（A） |
| 终端标题 OSC（"OC \| title"+开关） | GAP※ | app.tsx:572-615（A） |
| 颜色降档（256/16） | audit | renderer 层 |
| 图像渲染（cell 字符预览；无 sixel） | 部分 | 随 C 图像附件 |
| scrollback snapshot（mini 退场 banner） | GAP※ | A（mini 族） |

## G. 内容边界（R1-B 新族：按 payload 属性切分）

| 面 | 状态 | 锚/备注 |
|---|---|---|
| CJK/emoji 折行（ZWJ/VS16 宽=2） | GAP | composer 级有 golden（composer_cjk_input）；转录侧零 |
| 超长单行折行/无折行 | GAP | — |
| 空 assistant 消息 | GAP | — |
| 超快 delta 批次（流节奏边界） | GAP | — |
| 组合变音符/控制字符/内容中 ANSI 注入 | GAP | — |

## H. 独立前端与外部驱动（R1-A 新族归并）

| 面 | 状态 | 锚/备注 |
|---|---|---|
| mini 前端整套（footer 7 变体/scrollback/splash/verbosity） | GAP※ | cli/src/mini.ts→runMiniFrontend（**单端**：rust mini.rs 双实现） |
| plugin 自定义路由+route-missing 兜底 | GAP※ | plugin/render.tsx:50-65 |
| 外部远程驱动（tui.command.execute/toast.show/session.select/prompt.append） | GAP※ | app.tsx:1253-1276（A 新族） |
| 外部编辑器集成（VISUAL/EDITOR+zed sqlite） | GAP※ | editor.ts:14（A 新族；--fork/--replay 旗标单端） |
| 导出/分享（export-options/result） | GAP※ | ui/dialog-export-*.tsx |
| storybook（dev-only） | untestable | 生产不可达（OPENCODE_STORY） |
| devtools-bar（dev-only） | untestable | 同上 |
| 插件管理 UI（Plugins/Stats/Latex/Merman/DiffViewer 内置插件族） | GAP※ | feature-plugins/ |

## I. CLI 入口族（R3-1 新族：整族双端实装，差分+单元双可测）

| 面 | 状态 | 锚/备注 |
|---|---|---|
| 子命令树：upgrade/uninstall/acp/api/debug{agents,config,paths}/models/stats/reload/pair/serve/session{list,delete,export,import} | GAP | ref commands.ts Root Spec 全表；rust oc-cli/src/handlers/ 已全实装 |
| auth{list,login,logout,export,import,switch} | GAP | auth login 表单关联 C 族 FormPrompt |
| mcp{list,add,auth,logout} / plugin{list,add,check,update,remove} | GAP | rust handlers auth.rs/mcp.rs/plugin.rs |
| 根旗标：--standalone/--server/--auto/--yolo/--dangerously-skip-permissions/-c/-s/--prompt | GAP | rust cli.rs prompt_and_auto 仅单测，无场景级 |
| run 非交互输出（default/json、--thinking、--file、tool_render） | GAP | rust run/noninteractive.rs+tool_render.rs 独立模块 |
| oc-pty 守护进程接口（daemon/main） | audit | **单端域** |

## J. 组合矩阵（R3-3：输入×状态交叉，均 GAP※）

| 组合 | 备注 |
|---|---|
| resize × 权限/FormPrompt 打开中 | resize 族 6 文件均无 dialog 交叉 |
| 粘贴大文本/图像 × 模态打开中 | paste 队列×焦点抢占 |
| 并行多 tab 流式 × resize × 切换 | B 单轴已列，无交叉 |
| queued prompt 入队 × 中断（ESC）瞬间 | inbox×abort 竞态 |
| 迁移 overlay 进行中 × 任意输入/resize | E 族迁移 UI 无输入面 |
| composer tab 组 × 持久 PTY 输出到达 × 失焦 | C×B 互不交叉 |

## 维护规程

- 每波收口：控制者核对台账——新 covered/新 GAP/关闭的 GAP；GAP 超两波未补=波计划显式说明。
- 新可测面来源：用户真机报告（第一优先）、代码审读、上游 diff、复盘、审计轮。
- 审计轮次记录：R1=A+B 双独立审计（2026-11-18，A 族级 20+面级 47、B ~28+G 整族、订正 7 处）；R2=控制者合并（本版）；R3=对抗性攻击（待执行）；此后每两波至少一轮对抗审计。
- 优先级排序原则：用户真机已暴露>GAP※（有锚可建）>audit 定稿；族完整性优先（先补族的骨架场景再扩矩阵）。
