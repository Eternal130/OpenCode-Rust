# A2 结论: ref UI/CLI/AI 侧清单完成 — tui 61.4k / ai 30.8k / client 16.8k / cli 12.2k / schema 5.7k / protocol 4.2k / util 4.2k / theme 1.6k 非测试 LOC；46 providers、~200 keybinds、3 路由、36 内置主题、~130 client 端点、33 API groups

# opencode v2.0.24 参考实现 — UI/CLI/AI 功能清单（仅 packages/*/src 源码证据）

## 1. tui/src（SolidJS + @opentui/core 终端 UI）

### ① routes/（Route 类型仅 3 种 — context/route.tsx: `HomeRoute | SessionRoute | PluginRoute`）
- **home** — 起始页：logo + Prompt 输入、项目/会话选择入口（routes/home.tsx）
- **session** — 会话页（routes/session/index.tsx），子件：composer（context/index/shell-tab/subagents-tab/terminals-tab 四 tab，form.tsx 主体）、permission.tsx 权限提示、sidebar.tsx 侧栏、group-view/grouping/{session,tree} 工具调用分组、message-parts.tsx 消息渲染、message-navigation、anchors/anchor-view 锚点、activity-summary、history、rows、mount-budget（虚拟化预算）、render-context、thinking-syntax、location-missing、dialog-{execute,fork,message,timeline} 子对话框
- **plugin** — TUI 插件注册的路由页（app.tsx 中 `<PluginRoute>` 渲染 + PluginRouteMissing 兜底）

### ② component/（全部）
- command-palette.tsx — ctrl+p 命令面板
- devtools-bar.tsx — devtools 状态条
- dialog-agent / dialog-model / dialog-variant — agent 选择 / provider+model 选择 / 模型 variant 循环
- dialog-config / dialog-experiments — 设置项 / 实验特性开关
- dialog-debug / dialog-error-details / dialog-status / dialog-update — 调试信息 / 错误详情 / 状态总览 / 升级提示
- dialog-image-preview — 附件图片预览
- dialog-integration / dialog-mcp / dialog-skill — 集成账号管理 / MCP 服务器管理 / skill 选择
- dialog-open — 打开最近会话与项目（open.menu）
- dialog-pair — 设备配对（pair 链接）
- dialog-session-list / dialog-session-rename — 会话列表（pin/删除）/ 重命名
- dialog-shell-output — composer shell tab 的命令输出查看
- dialog-stash — prompt stash 管理
- dialog-theme-list — 主题切换
- dialog-workspaces / dialog-workspace-file-changes / dialog-worktree-name — workspace 管理 / workspace 文件变更 / worktree 命名生成
- error-component.tsx — 渲染错误兜底（带安全调色板）
- fade-in-text / shimmer-text / title-shimmer / masked-text — 文字渐显 / 微光 / 标题微光 / 打码
- logo.tsx — ASCII logo（logo.ts 数据）
- migration-overlay.tsx — 配置迁移遮罩
- one-cell-spinner.tsx + spinner.tsx + spinner-frames.ts + register-spinner.ts — 单元格 spinner 体系与帧定义注册
- panel-host.tsx — 侧栏/浮层 panel 宿主
- patch-diff.tsx — patch/diff 行内渲染组件
- plugin-route-missing.tsx — 插件路由缺失兜底
- prompt/{index,autocomplete,metadata,move,draft-stash,local-attachment} — 输入框本体 / 自动补全 / 元数据行 / 光标移动 / 草稿 stash / 本地附件
- reconnecting.tsx — 服务器断线重连提示
- retry-provider.tsx — 工具行内 retry span
- session-frame.tsx — 会话主框架（含拖拽焦点处理）
- session-tabs.tsx / session-tabs-rail.tsx — 会话 tab 条 / 侧向 rail
- startup-loading.tsx — 启动加载画面
- tab-pulse.tsx — 后台 tab 活动脉冲
- terminal-pane.tsx — 内嵌终端 pane（pty）
- theme-error-toast.tsx — 主题加载失败 toast
- use-connected.tsx — 连接状态 hook

### ③ ui/ 原语
- dialog.tsx + dialog-alert/confirm/prompt/select/help/export-options/export-result — 对话框框架与六种标准对话框
- file-path.tsx、link.tsx — 路径显示、超链接
- layout.ts、border.ts、animation.ts、subcell.ts、one-cell-motion.ts — 布局/边框/动画/子单元格运动
- pane-resize.ts + pane-resize-handle.tsx — 分栏拖拽调宽
- select-controller.ts、spinner.ts、toast.tsx、working-directory-actions.tsx — 列表控制器、spinner、toast、cwd 切换动作

### ④ prompt/（输入内容管线）
- parse.ts / codec.ts — 提示词解析（文件行范围等）/ 可编辑编辑态 `EditablePromptInput`
- content.ts / display.ts — 内容规范化 / 显示宽度与 offset 计算
- attachment.ts — 图片附件去重；part.ts — 粘贴文本占位符展开；mention.ts — @mention 重对齐
- traits.ts — `PromptMode = "normal" | "shell"`；history.tsx — 输入历史环 + PastedText
- frecency.tsx — 目录 frecency 排序（MAX 1000）；directory-completion.ts — slash 命令参数补全；directory-recents.ts — 最近目录；stash.tsx — prompt 暂存条目
- diff/patch 展示：util/diff.ts + component/patch-diff.tsx + feature-plugins/system/diff-viewer*.tsx（文件树/图片 diff/单 patch/分栏-统一视图）
- markdown 渲染：plugin/markdown.ts → `@opentui/core` `createMarkdownCodeBlockRenderer`，按语言注册 last-wins 代码块渲染器
- 语法高亮：parsers-config.ts — web-tree-sitter wasm parser + nvim-treesitter 查询（opentui 内置 md/js/ts，其余从 GitHub 拉 wasm）

### ⑤ config/（可配置项键名全集 — config/index.tsx Schema + keybind.ts）
- `keybinds`（BindingValue：字符串/KeyStroke{name,ctrl,shift,meta,super,hyper}/对象{key,event:press|release,preventDefault,fallthrough}/false/"none"/数组）
- `leader: { timeout: 2000 }`（默认 `<leader>` = ctrl+x）
- `mouse`（默认 true）、`cursor {style: block|underline|line|default, blinking}`、`new_location: launch|inherit`、`permissions: prompt|autoaccept`、`tps`（tokens/s 显示）
- `theme.mode: auto|on|off(light/dark)`、`theme.enabled`、`theme.scope: global|cwd`、`session.layout: horizontal|vertical`、`session.indicators: status|numbers`
- `attention {notifications, sound, volume, sound_pack, sounds}`、`plugins {package, options}[]`、`experimental {…}`
- **keybind 全集**（config/keybind.ts `Definitions`，约 200 条）：leader、app.{exit,clear,debug,console,scrap,toggle.{animations,file_context,diffwrap,paste_summary}}、command.palette.show、help.show、docs.open、opencode.{settings,status,debug}、server.pair、service.restart、location.reload、diff.*（open/close/down/up/page/half_page/first/last/next_hunk/previous_hunk/next_file/previous_file/toggle_file_tree/single_patch/switch_source/toggle_view/mark_reviewed/help + 5 条 deprecated）、prompt.editor、theme.{switch,switch_mode,mode.lock}、session.{sidebar.toggle,fork,rename,delete,share,unshare,interrupt,background,compact,aside,cd,queued_prompts,new,list,timeline,export,copy,copy.id,move,toggle.scrollbar,toggle.exploration_grouping,verbosity.cycle,child.{first,next,previous},parent,pin.toggle,page.up/down,line.up/down,half.page.up/down,first,last,message.next/previous,user.next/previous,messages_last_user,undo,redo,toggle.thinking}、session.tab.{next,previous,history.back/forward,next_unread,previous_unread,close,reopen}、queued_prompt.{undo,delete}、stash.delete、model.{dialog.provider,dialog.favorite,list,cycle_recent,cycle_recent_reverse,cycle_favorite,cycle_favorite_reverse}、mcp.list、provider.connect、agent.{list,cycle,cycle.reverse}、variant.{cycle,list}、messages.copy、prompt.{submit,queue,editor_context.clear,images.view,skills,stash,stash.pop,stash.list,clear,paste,history.previous/next,autocomplete.{prev,next,hide,select,complete}}、input.*（submit/newline/move/select/line.home/end/visual/buffer/delete/backspace/undo/redo/word/select.all 全套编辑键）、composer.{subagent.{up,down,select,interrupt},shell.{up,down,select,kill},terminal.{up,down,select}}、terminal.{select,toggle,close,suspend,title.toggle}、dialog.{select.*,prompt.submit,integration.{rename,delete},worktree.generate,move_session.{new,move,delete,refresh},mcp.toggle,plugins.{error,install,update,check}}、permission.prompt.fullscreen、plugins.{toggle,list,install}、open.menu

### ⑥ theme/（token 结构 — theme/src/tui/schema.ts 完整定义 + theme/component.ts 消费侧）
- 结构：`text {base, muted, action, formfield, feedback{error,warning,success,info}}`、`background {base, raised{base,high,max}, action, formfield, feedback}`、`border.base`、`scrollbar.base`、`diff {text/background{added,removed,context,hunkHeader}, highlight, lineNumber}`、`syntax`、`markdown`
- color 值：hex / 256 色 / ANSI named；stateful 定义（default/hover/active/focus 等）；resolve.ts 补全、system.ts 亮暗探测、discovery.ts 主题发现、v1.ts+v1 迁移
- 内置 36 主题资产（theme/assets/*.json）：aura, ayu, carbonfox, catppuccin(+frappe,macchiato), cobalt2, cursor, dracula, everforest, flexoki, github, gruvbox, kanagawa, lucent-orng, material, matrix, mercury, monokai, nightowl, nord, one-dark, opencode(+v2), orng, osaka-jade, palenight, rosepine, solarized, synthwave84, tokyonight, vercel, vesper, zenburn

### ⑦ mini/（`opencode mini` 极简前端）
- runtime.ts 总编排 + runtime.{boot,lifecycle,queue} — 并发启动解析（keymap/model catalog/history）、renderer+RunFooter 生命周期、prompt 队列循环
- stream.ts + stream-v2.{fragment,image,subagent,transport} — SDK 事件订阅 → scrollback 写入流（碎片/图片/子代理/传输）
- footer.{form,command,menu,permission,prompt,subagent,view,width,ts} — 分割 footer 的表单/命令/菜单/权限/提示/子代理各视图
- scrollback.{shared,surface,writer} — 回滚缓冲写入；splash.ts 启动画面；demo.ts 演示模式（--demo 隐藏 flag）
- session-data/session.shared/turn-summary — 会话数据与回合小结；tool.ts/tool.public — 工具行渲染；verbosity/variant/theme/catalog.shared/mono/types — 详细度、variant、mini 主题、目录类型

### ⑧ feature-plugins/（内置 TUI feature 插件）与 plugin/（插件机制）
- home/footer — 首页 footer；prompt/{btw,footer} — 输入框 hint/footer
- sidebar/{context,footer,mcp} — 侧栏上下文/footer/MCP 状态
- system/{diff-viewer + file-menu + file-tree + file-tree-utils + image, notifications, plugins, stats-data, stats} — 全屏 diff 查看器（含图片 diff）、系统通知、插件管理器、用量统计
- system/storybook/* — `OPENCODE_STORY` 环境变量驱动的组件故事页（index、footer、merman-layouts、spinner fixtures、session-tabs 等）
- plugin/ 机制：api.tsx（TUI 插件 API）、builtins.ts、context.tsx、discovery.ts（发现）、source.ts、structure.ts、watch.ts（热重载）、markdown.ts（markdown 代码块渲染器注册）、render.tsx（PluginRoute 渲染）、runtime-plugin-support.{bun,node}（运行时判定）

### ⑨ devtools/index.ts — createSignal 驱动的调试面板状态（app.debug/app.console 键）
### ⑩ editor 集成
- editor.ts — 外部 `$EDITOR` 打开 prompt/session export（编辑器发现、临时文件清理）
- editor-zed.ts + editor-zed-sqlite.{bun,node}.ts — 读取 Zed 的 SQLite 找已打开文件/最近项目做集成

### ⑪ 交互设施
- attention.ts + attention-sounds.{bun,node}.ts — 通知铃声/通知开关；audio.ts — @opentui/core Audio 播放
- clipboard.ts — createClipboard/createHostClipboard OSC52+宿主剪贴板
- util/selection.ts — 选区复制/按键处理；util/scroll.ts — CustomSpeedScroll 滚动加速；util/marquee.ts 跑马灯
- context/（Solid contexts）：Args, Client, Data, Epilogue, Exit, Leader(keymap), Local, Permission, PromptRef, Route, SessionTabs, SessionTerminals, Theme, UpdateNotification + attention/clipboard/editor/event/helper/interactivity/keymap/location/log/panel/path-format/runtime/storage/thinking/session-retention/session-tabs-model

### ⑫ app.tsx / runtime.tsx
- **solid-js + @opentui/solid**：`import { render, useRenderer, useTerminalDimensions } from "@opentui/solid"`（app.tsx:1），渲染目标为 @opentui/core 终端 Renderer；`render(() => <LogProvider>…)` 挂载（app.tsx:293）
- 生命周期由 Effect 驱动：SIGHUP→destroyRenderer、renderer "destroy"→Latch、主题 mode 握手后首帧；路由切换更新终端窗口标题（`OC | …`）
- runtime.tsx 为薄再导出层（startup/interactivity 等 context 供 useRoute/useTuiStartup）

## 2. cli/src（Effect-TS CLI，bun 运行）

### ① commands/commands.ts 命令树（Spec.make/Flag/Argument 定义）
- 全局 flags：`--print-logs`、`--standalone`（私有 server）、`--server <url>`、`--auto`、`--yolo`（hidden）、`--dangerously-skip-permissions`（hidden）
- 根 `opencode [directory]` flags：`-c/--continue`、`-s/--session <id>`、`--prompt <p>`（默认进 TUI，handlers/default.ts）
- `upgrade [target]` `-m/--method`；`uninstall` `-c/--keep-config -d/--keep-data --dry-run -f/--force`
- `acp` — 启动 Agent Client Protocol server
- `api <operation|METHOD path>` `-d/--data -H/--header(≤100) --param` — 对运行中 server 发 OpenAPI 请求
- `debug`：`agents`、`config`、`paths [db|home|data|config|cache|state|tmp|bin|log|repos]`
- `auth`：`list --format default|json`、`login [target] --method --answer k=v(≤100)`、`logout [target] [credential]`、`export [target]`、`import [file]`、`switch [target] [credential]`
- `mcp`：`list`、`add <name> [--url --header --env --global] [-- command…]`、`auth [name]`（OAuth）、`logout <name>`
- `plugin`：`list --builtin`、`add <package>`、`check [target]`、`update [target]`、`remove <package>`
- `models`、`stats --days --year --all --project --models --tools --cost --full --limit --json`
- `mini` `-c -s --fork --replay/--no-replay --replay-limit -m/--model --agent --prompt --demo(hidden)`
- `run [message…]` `-c -s --fork -m/--model provider/model#variant --agent --format default|json -f/--file(≤100) --title --thinking`
- `session`：`list -n/--max-count --format table|json`、`delete <sessionID>`、`export [session] --sanitize`、`import <file|url> --directory`
- `service`：`start|restart|status|stop`、`get [key] [name]`、`set <key> <value> [env-value]`、`unset`
- `reload` — 重载配置；`pair --url` — 打印浏览器/app 配对链接
- `serve` `--hostname --port --cors(可重复) --service --stdio` — v2 API+web server

### ② acp/（ACP 协议支持面，20 文件）
- agent/client/connection/child — agent 端 + 反向 client 连接与子进程；capabilities.ts — 解析 ClientCapabilities + 自定义能力 `opencode/child-session-updates`
- sessions/turn/prompt/content — 会话映射、回合驱动、prompt 模式（edit/agent 等）、内容块转换（translate.ts）
- permission.ts — ACP 权限请求映射；elicitation.ts — 表单 elicitation；compaction.ts — 压缩；replay.ts — 历史回放
- catalog.ts/config-option.ts — 模型目录与 config option（slash 命令/模型选择模式）；directories.ts、error.ts、tool.ts — 工具调用上报

### ③ mini.ts / mini-host.ts — mini 前端装配（服务连接+session 解析）与独立宿主（mini-host 内嵌 server 生命周期）
### ④ run/ — run.ts（主流程：stdin 读取、文件附件、session 续接/fork）、noninteractive.ts（859L 非交互 JSON/文本输出、事件流渲染、权限应答）、ui.ts（ANSI 样式常量）、v1.ts（v1 兼容）、index.ts
### ⑤ services/ — server-connection（后台服务发现连接）、standalone（私有 server）、service-{config,registration}（服务配置与注册文件）、updater(+action,+preflight)（自升级）、web-ui、retained-image
### ⑥ framework/ — spec.ts（Spec.make/Flag/Argument 封装 effect/unstable/cli）、runtime.ts（handlers 动态 import 装配 + 运行）
### ⑦ node/ — index（EPIPE 保护+plugin runtime 注册）、target.ts（平台三元组→node target）、plugin-runtime.{effect,promise}（插件加载双运行时）
### ⑧ 其他
- server-process.ts — serve 子进程 Effect 生命周期；ssh-askpass.ts — `OPENCODE_SSH_ASKPASS_PORT` 时作为 OpenSSH SSH_ASKPASS 程序（入口 index.ts 首行分流）
- app-assets.ts — web/app 资产定位；database-path.ts — 数据库路径解析
- config/{config,schema,migrate} — jsonc 读写 + immer draft + v1 迁移；session-target.ts — `-c/-s` 目标解析；cpu-profile/heap — 诊断信号；ui/{integration-picker,prompt,timeline} — 交互式辅助 UI

### ⑨ index.ts 入口 — Effect 分层装配：ssh-askpass 分流 → Handlers 懒加载表 → Heap/CpuProfile listen → uncaught 兜底 → Observability(OTLP) → Config/Updater/Global/Npm/EffectFlock layers → Runtime.run(Commands, Handlers)

## 3. ai/src（provider/协议无关生成层）

### ① providers/（46 个 provider 定义文件；providers.ts=再导出；provider.ts=`Definition.make{id, model, apis?}` 工厂）
- 走通用/单协议 chat：baseten、cerebras、cloudflare-workers-ai、deepinfra、deepseek、digitalocean、fireworks、togetherai（openai-chat）；venice（venice-chat+openai-chat）；cohere（cohere-chat+openai-chat）；groq/openrouter（openai-chat + shared 协议工具）
- openai 系：openai.ts（openai-chat+openai-responses）、openai-compatible.ts（openai-compatible-chat）、openai-compatible-responses.ts、openai-options.ts / open-responses-options.ts（连接键声明）
- anthropic 系：anthropic.ts、anthropic-compatible.ts（anthropic-messages）
- google 系：google.ts（gemini + google-interactions）、google-vertex.ts（gemini）、google-vertex-{chat,messages,responses}.ts（openai-chat / anthropic-messages / open-responses on Vertex）+ google-vertex-shared.ts
- 特判/专有协议：amazon-bedrock.ts（bedrock-converse + 事件流鉴权）、amazon-bedrock-mantle.ts（anthropic-messages/openai-chat via mantle）、azure.ts（openai-chat+responses）、alibaba.ts（alibaba-chat/messages/responses）、meta.ts、minimax.ts、moonshot.ts（anthropic-messages+openai-chat）、mistral.ts（mistral-chat）、zai.ts + zai-coding-plan.ts（zai-chat/messages + zai-images）、xai.ts、cloudflare-ai-gateway.ts（gateway-protocol）、opencode-zen.ts、typesafe-ai.ts（结构化测试 provider）
- 多模态媒体 provider：assemblyai/deepgram/elevenlabs（transcription+speech）、cartesia（speech）、google-speech/transcription 经 google.ts、black-forest-labs（bfl-images）、fal（fal-images+video）、replicate（replicate-images）、runway（runway-video）、stability（stability-images）、xai-images/video

### ② protocols/（协议适配器，70 文件）
- 语言：openai-chat、openai-responses、openai-compatible-chat/responses、anthropic-messages、gemini、google-interactions、bedrock-converse、bedrock-event-stream、mistral-chat、cohere-chat、venice-chat、zai-chat/messages、alibaba-{chat,messages,responses}、meta-{messages,responses}、open-responses(+channel,+continuation)
- 媒体：openai-images/speech/transcription、google-images/speech/transcription/video、bfl-images、fal-images/video、replicate-images、runway-video、stability-images、xai-images/responses/video、zai-images、assemblyai/deepgram/elevenlabs-{speech,transcription}、cartesia-speech
- utils/：bedrock-{auth,cache,media}、cache、claude-model、fal-queue、gateway-protocol、gemini-{generate-content,json-schema}、lifecycle、media-input、mistral-tool-id、openai-options、open-responses-options、partial-json(+options)、responses-{checkpoint,compaction,hosted-tools}、speaker-turns、speech-stream、tool-schema、tool-stream

### ③ 生成管线
- llm.ts（196L）— 用户入口：`generate`（Effect）/`stream`（Stream<LLMEvent>）/`request`（编译 LLMRequest：tools→ToolEntry、toolChoice）/`generateObject`（强制合成工具 `return_structured_result` 走工具调用回传结构化输出，兼容不支持原生 JSON 的 provider）
- generation.ts（207L）— 异步生成任务对象 `Generation`：Status queued/running/completed/failed/cancelled/expired、QueuedEvent/ProgressEvent 轮询（默认 5s 间隔/10min 超时）、resultEvents
- ai-client.ts — `layer`/`layerWith(executor)`：以 RequestExecutor 为注入点的 AI 服务层（默认 fetch executor）
- route/ — 协议路由：executor.ts（请求执行+错误体透出）、endpoint.ts、framing.ts、protocol.ts、media.ts/media-protocol.ts、auth.ts/auth-options.ts、client.ts；transport/http.ts（fetch）、transport/websocket.ts + websocket-channel.ts（WS 通道，526L）
- abort/重试/代价：provider-error.ts — `retryable` 判定（rate limit/服务端错/未送达传输错）；schema/events.ts 含 usage/token 事件；cache-policy.ts（见④）；cost 在 usage（schema/llm.ts + session-stats 上层）

### ④ 其余核心文件
- cache-policy.ts — 编译期把 `LLMRequest.cache` 策略注入 CacheHint 到指定 part（协议无关，先于 per-protocol 处理）
- effort-updates.ts — reasoning effort 变更的 prompt cache 失效策略（原生 per-message 更新 vs 剥离 Message.effort 标记）
- tool.ts — `Tool.make{parameters,success,execute,toModelOutput}` schema 编解码工具定义 + `toDefinitions`；tool-runtime.ts — 会话内工具执行运行时；tool-history.ts — 工具历史压缩/重放整理
- providers.ts/index.ts — provider 注册面；protocols.ts — 协议再导出

### ⑤ 多模态客户端
- image.ts/image-client.ts — ImageModel：生成/编辑接口；speech.ts/speech-client.ts — TTS；transcription.ts/transcription-client.ts — STT；video.ts/video-client.ts — 文生视频（多为异步 job：route→poll→await）
- media.ts — `Media.Source` 可序列化媒体资产源；media-client.ts — 统一媒体客户端；media-model.ts — 媒体模型基类（ids/route/http overlay）

### ⑥ route/（见③）；⑦ schema/ — ids（ProtocolID/ModelID/ProviderID/RouteID/ContentBlockID/ToolCallID）、messages（LLM 消息+CacheHint+effort 标记，449L）、events（LLMEvent 全集：text/thinking/toolCall/usage/…，742L）、errors（AIError 结构，177L）、options（per-provider 连接与请求选项，347L）
### ⑧ testing.ts — 固定脚本回放的测试 executor（259L）；⑨ provider-error.ts（retryable 分类）/provider-package.ts（provider 包级设置：每入口声明连接键，其余键为请求选项）
- experimental/ — evaluation-client + evaluation（评测）+ system-one；utils/ — bytes/json/media-type(魔数探测)/record/sanitize

## 4. client/src（客户端 SDK）

- service.ts — 本地服务生命周期：`Endpoint{url,auth?:basic}`、`DiscoverOptions{file?,version?}`、`EnsureOptions{command?,env?,onStart?}`、`StopOptions{pty: clear|handoff}`、`Info{id,version,url,pid,password?}`（注册文件 XDG state）
- contract.ts — 从 `@opencode/protocol/client` 再导出 `ClientApi, effectOmitEndpoints, groupNames, promiseOmitEndpoints`
- effect/ 与 promise/ 双 API 面（generated/client.ts 按 protocol groups 生成 ~130 端点方法：session.*、message.*、fs.*、shell.*、pty.*/persistentPty.*、permission.*、provider/model/agent/command/skill、mcp.*、integration.*、credential.*、vcs.*、worktree.*、websearch.*、form.*、rpc.call、event.subscribe、server.info/connect…）
- rpc-runtime.ts — RPC 调用/事件订阅运行时（schema 校验 read/readError/event/eventType）；shared-events.ts — `make<A>` 泛型事件订阅器
- transport — fetch（effect api）+ `event.subscribe` SSE 事件流；rpc.call 走 RPC 通道；pty-handoff.ts — service stop 时 pty 终端 clear|handoff 处理
- service-probe.ts — 端点探测（默认时序）；service-contender.ts — 服务进程竞争检测/清理（spawn ChildProcess）；service-timing.ts — `EnsureTiming{pollInterval,requestTimeout,spawnDelay}`；service-version.ts — 版本比对
- solid/ — Solid 绑定：data.ts（1952L：server 事件投影进 Solid store，串行 revalidate、断线失效）、connection.ts（连接 store）、pty.ts（持久终端客户端）、index.ts

## 5. theme / util / schema / protocol

### theme/src/tui（1,592L）
- schema.ts — 主题 token Schema（见 1⑥）；resolve.ts — 部分→完整主题解析；expand.ts — 颜色引用展开；color.ts — 颜色解析；select.ts — 主题选择；syntax.ts — 语法 token→颜色映射；v1.ts+v1-migrate.ts — v1 主题迁移；types.ts — 公共类型

### util/src（4,566L，测试 407）
- patch.ts（diff/patch 应用）、glob.ts、fs-util.ts、path.ts(+test)、encode.ts(+test)、binary.ts(+test)、bom.ts(+test)、artifact.ts(+test)、activity-calendar.ts(+test)、hash.ts、flock.ts/effect-flock.ts（文件锁）、open.ts(+test)（跨平台打开 URL）、retry.ts(+test)、npm.ts/npm-config.ts（npm 生态操作）、process.ts（AppProcess）、global.ts/global-roots.ts(+workerd)、runtime/（bun/node/workerd 条件导入）、runtime-import.ts、effect/（app-node(+platform)、layer-node、memo-map、runtime、service-use）、observability/（logging、otlp OTEL 导出）、session-title-fallback.ts、cross-spawn-spawner.ts

### schema/src（5,693L，TypeBox/Effect Schema 域模型，~80 文件）
- 核心域：session(+v1,+metadata,+message,+inbox,+stats,+fork,+revert,+compaction-event,+error,+provider-context,+status-event,+transfer,+id)、event/event-log/event-manifest/legacy-event/durable-event-manifest、permission(+v1,+saved)、tool、prompt(+input)、message、mcp(+event)、lsp-event、location(+event)、models-dev/model/llm/provider、money/token-usage、file-diff、reference、form、question、skill、command、agent、config(+config/ 子目录 19 文件：agent,command,compaction,experimental,formatter,lsp,mcp,media,model,plugin,policy,provider,reference,shell,tool-output,warming,watcher,websearch,worktree)、workspace(+event,+id)、worktree(+event)、project(+id)、credential、integration(+id)、instruction(+entry)、pty(+persistent,+ticket)、shell、snapshot、vcs(+event)、websearch、server-event、tui-event、installation-event、filesystem(+v1)、v1/、schema.ts（基础类型：AbsolutePath/RelativePath/NonNegativeInt/DateTimeUtcFromMillis…）、identifier.ts、rpc.ts
- 特点：Effect Schema + annotate 描述 + v1 legacy 迁移层

### protocol/src（4,240L）
- api.ts — 汇总 33 个 HttpApiGroup（server.{agent,command,config,credential,debug,event,experimental,form,fs,generate,integration,location,message,migration,model,mcp,pair,permission,plugin,project,provider,pty,persistentPty,reference,rpc,session,shell,skill,vcs,websearch,worktree,server}）；endpoint 命名 `<group>.<action>`（如 session.prompt、session.permission.reply、fs.read、pty.connect.token、event.subscribe、rpc.call、experimental.* 共 13 个实验端点）
- client.ts — ClientApi/promise 与 effect 双面映射 + groupNames；errors.ts — 错误集（SessionNotFoundError/SessionBusyError/InvalidCursorError/FormInvalidAnswerError/…）；middleware/{authorization,schema-error}；simulation.ts — 仿真端点支持

## LOC 分布（src，.test.ts(x)/fixtures 算测试）

| package | 非测试 LOC | 测试 LOC | 备注 |
|---|---|---|---|
| tui/src | 61,400 | 0 | 2 个 spinner fixtures 文件(259L)单独计 |
| ai/src | 30,782 | 0 | 最大协议面 |
| client/src | 16,766 | 0 | solid/data.ts 1,952L 为最大单文件 |
| cli/src | 12,232 | 158 | 测试：ssh-askpass/api/updater |
| schema/src | 5,693 | 0 | |
| protocol/src | 4,240 | 0 | |
| util/src | 4,159 | 407 | 8 个 *.test.ts |
| theme/src | 1,592 | 0 | |
| **合计** | **136,864** | **565** | |
