# T1 结论: oc-tui 对 ref v2.0.24 TUI 域是"语义级重实现 + 逐处标注"的高保真移植——渲染/输入/剪贴板/attention/editor/生命周期主链路逐条对齐且大量被 golden 与逐字节测试钉死；残留差异集中在少数边角（标题 fallback、剪贴板宿主写臂、SIGHUP 优雅退出、选区 ctrl+c 吞键、stdin 双读竞争）。

## 对比表

| 域 | ref（锚定） | rust（移植） | 判定 |
|---|---|---|---|
| 挂载/生命周期 | app.tsx:204-451 `render(()=><LogProvider>…)` 挂载到 @opentui/solid renderer；acquireRelease destroyRenderer；`renderer.once("destroy")→Latch` 优雅收尾；exit reason/epilogue 写 stderr/stdout | app.rs:7953-10494 `run/run_with_size`：SessionTerminal 获取（raw+alt screen+模式 burst）→ pump loop → teardown epilogue（kitty pop、graphics delete、destroy 驱动 session teardown、exit epilogue :10477-10491） | ✅（SIGHUP 差异见 D-3） |
| 首帧门 | `waitForThemeMode(1000) ?? "dark"` 后才 render (app.tsx:290)；handoff 首帧 `FRAME→complete` | boot discard window 300ms（`BOOT_DISCARD_WINDOW` app.rs:7929）+ 阻塞 sleep 保持"首帧先于 transcript sync"时序 (app.rs:8832-8846)；theme mode 由 probe 归约，默认 Dark | ✅（机制不同、语义对齐） |
| 标题 `OC \| …` | app.tsx:596-622：home/未命名→"OpenCode"，`isFallbackTitle`→"OpenCode"，>40 截 37+…，plugin 路由 `OC \| {name}` | store.rs:5148-5172：同截断；OSC 0 变更才发 (app.rs:10308-10318)；terminal_proto.rs:126-128 BEL 终止符 | ⚠️ D-2（fallback regex 与 plugin 路由缺失） |
| reconnecting | app.tsx:1294-1315：reconnecting 1s / 初始 5s 定时器 | store.rs:12370-12388 同 1000/5000 阈值；overlays_solid.rs:121-249 scrim+box | ✅ |
| 渲染循环 | solid 响应式树 + 光栅 diff（targetFps 60，raster 自有 diff/发射） | 自绘 `Frame`（paint.rs Buffer/ratatui 语义）→ `OpentuiEmitter`（render/opentui_emit.rs，opentui-core `emit_frame_sgr` + `?2026` 同步包裹 + 双光标策略 + OSC-8 链接 id 池）；脏判定 = redraw generation ∨ clock>0（app.rs:10134-10152）；golden 双格式比对 | ✅（架构差异被差分门钉平） |
| 脏区/diff | raster 行级 diff | poke→current/next buffer diff，`touched_rows` 簇感知 (opentui_emit.rs:341-374)；resize 换全量基线 | ✅ |
| ANSI 规范化 | @opentui/core SGR | ansi.rs:12-78 `serialize_canonical`（每行单一全状态 SGR、宽簇续格不发、行尾 reset）+ emit_fg/bg/apply_attributes 与 opentui-core 字节形一致 | ✅ |
| 颜色系统 | raster 发射期降级：COLORTERM/WT_SESSION/SSH→truecolor，`*256color`→256 索引，无 16 色降级 | color_mode.rs:69-99 同判定表；quantize 平方差最近、低索引破平（151-167），144 色 sweep 0 失配；帧模型保 truecolor、绘制 seam 降级 (app.rs:10265-10267) | ✅（经验推导，见 R-4） |
| 键盘/鼠标/粘贴/焦点 | CliRendererConfig `useKittyKeyboard:{}`（setup 即推）、useMouse、exitOnCtrlC:false | input_term.rs KeyPipeline（鼠标先、bracketed paste 收集、`\x1b[I/O` 焦点、lone-ESC chunk 尾冲、CSI-u 无条件解析）；kitty push `CSI>5u` 探测门控 (app.rs:8757-8784)；鼠标四模式 `?1000/1002/1003/1006` (terminal_proto.rs:57-60) | ✅（kitty 时序差异 D-5） |
| resize | renderer resize | 输入线程 50ms `check_resize` + 渲染前 winsize 实时对账（SE-F5 0x0 防退化，app.rs:10163-10180） | ✅ |
| 选区 | util/selection.ts + renderer 内建选区（多击历史、selectedRenderables） | selection.rs 矩形胞格模型 + app.rs:674-685/1266 右键与 release；`getClipboardText` 的 pasted 占位展开有 (app.rs:479-492) | ⚠️ D-4/D-6 |
| 滚动加速 | util/scroll.ts（MacOSScrollAccel / CustomSpeed(3)） | scroll_accel.rs 常数逐字对齐（150ms streak、6ms 下限、1+0.8(e^{v/3}−1)、上限 6） | ✅ |
| 跑马灯 | marquee.ts（grapheme 循环 + `·` GAP） | session_tabs_solid 80ms 边界钉扎时钟（app.rs:9989-10023 注释详述修复过程） | ✅ |
| 剪贴板写 | clipboard.ts:53-62 `all-available`：host 真臂（pbcopy/xclip…）+ OSC52 | clipboard.rs Osc52Sink：host 写臂 Unsupported（仅 OSC52），100KiB 拒绝、NUL 剥离、空写直发 | ⚠️ D-1 |
| 剪贴板读 | host 工具优先 + OSC52 查询窗 | terminal_proto.rs:441-538 同层叠 + 魔数嗅探；错误三分支文案逐字复刻 | ✅（并发见 R-1） |
| attention | attention.ts:189 行全语义（focus 门、normalize、volume、候选回路） | attention.rs:1-605 逐函数对齐 + 测试矩阵；`AttentionWhen/SoundName/SkipReason` 全集 | ✅ |
| 音频 | audio.ts opentui Audio（进程内解码） | sound.rs：Linux 进程内 opentui_core::audio；非 Linux ➕ 系统播放器 spawn 镜像（pw-play/paplay/ffplay/mpv/canberra，音量映射钉死） | ✅+➕ |
| OS 通知 | renderer.triggerNotification（native 三协议） | os_notify.rs osc9/osc777/osc99 + env 覆盖 + 载荷消毒 | ✅（auto 判定是镜像近似，注释自认） |
| bell | v2 从不发 `\x07` | ➕ `attention.bell`（默认 false）opt-in (app.rs:9155-9166) | ➕ |
| editor $EDITOR | editor.ts:14-42（VISUAL‖EDITOR，无兜底；空文件→undefined；非零留旧稿；finally 清理+resume） | editor.rs:80-132 逐条复刻（含 existsSync cwd 门 G-273④） | ✅ |
| Zed/IDE 集成 | editor-zed.ts + context/editor.ts（~/.claude/ide/*.lock ws、selection_changed/at_mentioned） | editor_zed.rs（SQL/评分/字节偏移→索引）+ editor_ide.rs（env 覆盖、authToken 头、1s 轮询） | ✅ |
| suspend/resume | app.tsx:1119-1129 suspend→SIGCONT resume→kill(0,SIGTSTP) | suspend.rs 信号纪律复刻 + ➕ 孤儿组 2s 有界等待；重建 session terminal + invalidate_full | ✅+➕ |
| storage | context/storage.tsx `<state>/<ch>/tui/<key>.json`（flock+fs.watch） | storage.rs 同路径契约（stable 通道 + local→stable 迁移 ➕；无跨进程 flock/watch，文档化放弃） | ✅（D-7） |
| 上下文体系 | 30 个 Solid context（Args/Client/Data/Route/Theme/…） | store.rs 信号面 + ~30 个 sink trait（Http*Sink 一一对应）+ reactive.rs 微型响应式运行时；`debug_assert_production_sinks` 保证装配 | ✅（结构性映射） |
| ui 原语 | dialog.tsx（modal 层、esc/ctrl+c、backdrop mouseup 关闭、refocus）、六种 dialog、pane-resize（300ms 边界双击复位）、select-controller、toast（暂停/恢复/队列/pending）、spinner/subcell/one-cell-motion | app.rs pump 的对话框层序注释 (app.rs:1-60) 与实现；pane_resizing/pane_boundary_click（:145-153）；toast hover 暂停 (:590-605)；marquee/spinner/one-cell 对应物在 components/*_solid；`dialog.clear` vs pop-one 为已裁定偏差 | ✅（D-9） |
| model-preference | model-preference.ts（flock + fs.watch + 原子写） | store ModelRing + mini/model_pref 仓储共享 `<state>/tui/model.json`（app.rs:8271-8275，脏时批量落盘） | ✅（无 watch） |
| 图片 | opentui native ImageFrameEmitter（kitty/sixel/blocks） | image_view.rs 桥接 opentui-core + image_codec.rs 自带 PNG 解码（sixel 路径）；probe 驱动 auto | ✅ |
| session.environment | app.tsx:489-499：`variables===undefined` 则**不发** | app.rs:9041-9063：总是 PUT 全量 `std::env::vars()`，每会话一次 | ⚠️ D-13 |
| mermaid/latex | 无对应物 | render/mermaid/{flowchart,sequence,state,gantt,gitgraph,timeline}、render/latex/{parser,layout,symbols} | ➕ rust 独有 |
| replay/farewell/stderr_redirect/crash-restore | replay 无；farewell=presentation.ts 出口 | replay.rs 脚本回放➕；farewell_solid.rs 字节对齐双实现；stderr_redirect.rs➕；terminal_proto.rs crash-restore➕ | ✅/➕ |

## 行为差异清单

- **D-1 剪贴板宿主写臂缺失**：ref `write({destination:"all-available"})` host 首选（clipboard.ts:15-18 真平台臂）+ OSC52；rust `UnsupportedHostBackend`（clipboard.rs:219-236）只发 OSC52。OSC52-incapable 本地终端上 ref 复制成功、rust 失败并弹错误 toast。
- **D-2 终端标题 fallback**：ref `isFallbackTitle(title)→"OpenCode"`（app.tsx:610-613，regex `^(New|Child) session - …Z$`）；rust store.rs:5148-5172 无此过滤——时间戳兜底标题渲染成 `OC | New session - 2026-…Z`。且 ref plugin 路由有 `OC | {name}` 臂，rust 插件页标题不变。
- **D-3 SIGHUP 语义**：ref SIGHUP→destroyRenderer→Latch→**优雅收尾**（exit reason/epilogue 写出，app.tsx:283-288,442-450）；rust `signal_restore_handler→crash_exit`（terminal_proto.rs:362-404）write(2) 恢复序列后 `_exit(1)`——无 epilogue、无 reason 输出。
- **D-4 选区 ctrl+c（copy-on-select 模式）**：ref selection.ts:78-87 首臂 clearSelection 后 `return`（**不 preventDefault**，击键继续下传）；rust `ClearAndConsume` 吞键（app.rs:1827-1831, selection.rs:158-168）。另 ref 有 `editing`（聚焦编辑器内选区交给编辑器）与 `selectedRenderables` 焦点扩展语义，rust 无编辑器内选区概念。
- **D-5 kitty keyboard 时序**：ref `useKittyKeyboard:{}` 在 renderer setup 即推 flag；rust setup flags=0，`CSI ? u` 探测应答后才推 `CSI >5u`（app.rs:8757-8773）——探测回包丢失的 kitty 终端上 rust 不启用增强键盘。
- **D-6 选区模型**：ref 选区由 renderer 管理（多击历史、cell 细分、跨 renderable）；rust 自绘胞格矩形近似（selection.rs:37-50 注释自认）。
- **D-7 持久化无跨进程同步**：storage.rs / model-preference 无 flock + fs.watch；配置热重载为每 tick mtime 轮询（~100ms，app.rs:10059-10133）vs ref 目录监听。
- **D-8 帧率节奏**：ref targetFps 60 光栅调度；rust 泵循环 ~100ms 轮询 tick（marquee 80ms 例外）——动画帧率上限不同（golden 无此维度）。
- **D-9 对话框关闭粒度**：rust esc/ctrl+c **pop-one**，ref `dialog.clear()` 清空整个栈（app.rs:709-714 注释明示为已裁定保留偏差）。
- **D-10 鼠标命中**：ref 完整 renderable 树命中/冒泡；rust 手工 hit-rect 路由表（app.rs:505-510 注释）——结构性差异，逐表面已对齐。
- **D-11 session.environment**：ref 仅当宿主提供 `input.environment` 时 PUT（app.tsx:495）；rust 无条件 PUT 全量进程 env（app.rs:9051-9053）——多上传数据面，隐私/体积差异。
- **D-12 rust 独有面 ➕**：`attention.bell`、非 Linux 播放器镜像、stderr 重定向、crash-restore、suspend 孤儿组有界等待、replay harness、mermaid/latex 渲染器、`*_solid.rs` 双实现、storage local→stable 迁移、OSC-8 链接 rect 发射。

## 正确性风险

- **R-1（高）stdin 双读竞争**：`clipboard_read_osc52`（terminal_proto.rs:474-519）与 palette 查询在**输入线程存活期间**直接 poll/read fd 0；输入线程 (app.rs:8717-8830) 同时 poll/read 同一 tty fd。OSC52 应答可能被输入线程吞成垃圾键序列，或剪贴板窗口偷走用户击键。palette 探测在输入线程启动前（安全），OSC52 查询路径没有同等串行化保证。
- **R-2 D-2 直观后果**：时间戳兜底标题污染终端 tab 标题（`OC | New session - …`）。
- **R-3 D-1 后果**：无 OSC52 支持的本地终端复制必然走错误 toast；`messages.copy` 等路径成功率低于 ref。
- **R-4 颜色量化系经验推导**：color_mode.rs:31-35 自述"正向映射不可从 ref 源码表达"，来自 0.5.14 raster 实证；上游 raster 升级会静默漂移（已 144/144 钉住，但只在被测色域）。
- **R-5 簇发射字节分歧**：多码点簇 deferred pass 与上游 inline 图素路径帧字节不同（opentui_emit.rs:33-38 已声明有界；golden 语料无簇）。
- **R-6 D-4 吞键**：copy-on-select 下的 ctrl+c、manual 模式残留选区期的 ctrl+y 会在选区存在时屏蔽同键的其它全局绑定。
- **R-7 boot discard window**：启动后 300ms 内非应答输入被丢弃（app.rs:8695-8703）——极快打字者首键丢失。
- **R-8 SIGHUP `_exit(1)`**：挂断时 exit code 恒 1 且无 epilogue（D-3），对脚本化包装可见。
- **R-9 配置重载丢失区间**：mtime 轮询粒度 ~100ms 内的连续两次保存只见最终态。低危。
- **R-10 `attention.notifications` auto 判定**是 TERM/TERM_PROGRAM 启发式（os_notify.rs:62-79），native raster 为进程内协商——个别终端下通知协议选择可能不同。

## 覆盖声明

- **已逐行读完**：ref 侧 app.tsx、clipboard.ts、attention.ts、audio.ts、attention-sounds.bun.ts、editor.ts、editor-zed.ts、model-preference.ts(前 60 行+rust 对应全链)、util/{selection,scroll,marquee}.ts、ui/{dialog,pane-resize,toast}.tsx；rust 侧 ansi.rs、color_mode.rs、paint.rs、frame.rs、opentui_emit.rs、input_term.rs、selection.rs、scroll_accel.rs、clipboard.rs、attention.rs、sound.rs、editor.rs、suspend.rs、os_notify.rs、terminal_proto.rs 全文，app.rs 源码段（1-10564）中的核心面（App 状态/apply_config/鼠标路由 80-1840、run loop 7860-10564），store.rs 的 title/reconnecting/route/plugin_page/model-ring 等定点段，image_view/replay/farewell_solid/editor_zed/editor_ide/storage 头部与关键注释。
- **抽样+代码注释交叉验证（未逐行）**：store.rs 其余 ~13k 行、layout.rs/presentation.rs/style.rs/wrap.rs/reactive.rs、image_codec.rs、sound/audio.rs、stderr_redirect.rs、ref 的 30 个 context/*.tsx 与 dialog-* 六种/select-controller/spinner/subcell/one-cell-motion/border/layout/link/file-path——这些文件在 rust 侧对应物中带有成体系的 ref 行号引用，抽验引用点（pane-resize 300ms、toast hover 暂停、marquee 80ms、dialog esc/ctrl+c、leader 拦截序、app.exit 空草稿门）未发现失实。
- **测试资产仅按需查阅**。mermaid/latex 已按 rust 独有标注（➕）。未覆盖：components/ 下 ~45 个组件逐行比对（T2/T3 分工）；mini/ 前端（T4）。
