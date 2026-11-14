# T9 W4 — 终波：dialog/palette 风格族 12 diverge 清零 + golden 重导 + 终局验证（通道 8）

基线 b2688800（W3 后：28 exact / 12 diverge）→ W4 终态：**40 case 全 EXACT，
gate exit 0，无新 G-ID**。oc-tui 1657/1657 全绿；workspace fmt/clippy/guards
三绿；通道连跑 3 次幂等（报告字节相同）。

## 1. 余 12 diverge 逐族归因（全部 = rust 修复面，无一放宽）

### 1a. scrim 复合语义（models-dialog ×2、sidebar-toggle ×2、各 dialog 的
### 遮罩下格残余）

双侧网格 + ref 源码取证的 ref 引擎经验语义（跨 10 个网格一致）：

- **bg 恒压暗**（rust 原已对）；**字形格 fg 同样压暗**——Indexed 需先经
  `palette_rgb` 解析成 rgb 再混合（rust 原 Indexed 直通 = 分歧：muted 128 在
  scrim 下应为 53）；default 白 materialize 为 dim(255)。
- **whitespace 格在 scrim 下失去 fg 绘制**：有样式的空格（238/bold/muted）
  回终端默认白且 attrs 剥离——bold 标题（"Models dialog"）的词间空格在
  对话框打开时读 `attrs=[]` fg=255，无对话框时两侧同 bold（session-text
  exact 佐证，同 footer 逐案对照）。`Default` 与 `Indexed(15)` 两种默认白
  形态**原样保留**（canonical 字节不动的路径不受影响——worktree 金帧
  38;5;15 与 command-palette 金帧 38;2;255 并存的两代形态由此兼容）。

实现：`dialog.rs blend_scrim`（backdrop 150/255）与 `chrome_solid.rs
DimBg`（sidebar 70/255，session-frame.tsx:385-397）共享。sidebar 侧实证：
ref 遮罩下转录字形 fg = rust fg ×0.7255 逐通道成立（238→173、128→93、
30→22、blue 245→178）。

### 1b. command-palette suggested 集（×2）

ref 空 filter 列表 = `[...suggested, ...options]`（command-palette.tsx
:52-66）。rust 缺：`session.share`（suggested: route=session，
routes/session/index.tsx:893）与 `session.list`（suggested:
data.session.list().length>0，app.tsx:703）；顺序上 session-route 命令经
`prependCommandResolver` 前置（share 排在 Switch session 前）。补
`suggested()` 两臂 + registry 把 session.share 移到首位。

### 1c. DialogStatus 自定义树（×2）

ref dialog-status.tsx **不是 DialogSelect**：paddingLeft 2 的 bold 标题 +
esc、无过滤输入、无 server-backed 标记行、高度随内容。rust 原以
SelectProps（带 Search 输入 + "(server-backed list — fetch in flight)"
标记）渲染。新增 `dialog_solid::Family::Status`（Pair 先例），行数据经
props.hints 递送。

### 1d. DialogOpen（×2）

- placeholder：ref "Search sessions and projects…"（:339）vs rust "Search"。
- rust 静息帧残留 "Refreshing sessions and projects… (…)" 行：根因是
  **diff driver 未装 project sink、也未发 boot ProjectList**（app.rs:8678
  的 live-loop boot 块）——`open_menu_pressed` 重置 Loading 后再无请求，
  永不回落。driver 补 `DriverProjectSink` + boot `LoopRequest::ProjectList`
  + `LoopFetch::ProjectList` 应用臂（镜像 app.rs:9834-9837）。/api/project
  返回 [] → Loaded → Projects 组与标记行在静息帧消失，与 ref 一致。

### 1e. Sessions 对话框 chrome（session-tabs ×2）

- **size large**（onMount `dialog.setSize("large")`，dialog-session-list
  :190；rust 原 Medium 60 → 78 clamp，标题/内容整体左移到 c5）。
- **pin/unpin 动作在多 tab 模式隐藏**（`hidden: sessionTabs.enabled()`，
  :250-251）；补 **rename ctrl+r** 动作；**右侧 footerHint** "all projects
  ctrl+a"（:288-291；`footerHints side:"right"`，非 action 形态=bold 标题 +
  base 色分隔空格 + muted label——dialog-select.tsx:626-633）。SelectProps/
  DialogInputs 增 `footer_hint`，select footer 右盒渲染。
- storage `all_projects` 默认改 false：ref `initial: config.tabs?.scope !==
  "cwd"` 且 resolved scope 默认 "cwd"（config/index.tsx:328）→ false；rust
  原默认 true 与 ref 相反（该字段此前无消费者，翻转无行为外溢）。
- pty f15 长标题裁剪断言随 large 盒更新（stage-1 `…` 在 cell 61：head +
  37 m）。

### 修复路径上的工程坑（记录）

`box_().child()` 是**单槽 setter**——footer 加右盒时用第二个 `.child()` 会
把左动作盒整个替换掉（症状：所有 select 的动作行消失）。改为
`.children(Children::Static(vec![left, right]))`。

## 2. golden 重导（用户 2026-11-14 批准）+ surf1 裁定

- `PARITY_UPDATE=1 cargo nextest run -p oc-tui screen_golden_tests`：
  3 个批准项（session-error-part / session-sidebar-live(w80+w160) /
  session-slash-palette）重导，14/14 复绿。
- **几何抽验（防自证）**：新 golden 的 composer rails r17-20、╹ r21、
  footer r22（=h−2）、r23 留白——与通道 8 **ref 腿** session-empty 网格
  逐点一致（独立于 rust 输出的第三方网格）。
- **surf1_detail_states**：本地 fixture pin vs ref oracle 冲突 → **判 ref
  oracle 对**（rail fg=background.base、bg=raised；ref session-tool-failed
  捕获 + BlockTool 源码，W3 已取证）。本地 pin 陈旧（W3 修正渲染前自录），
  按 oracle 重钉属常规维护。重钉后 rail 字节 `fg=10,10,10 bg=20,20,20` 与
  ref 网格逐字相等。
- 同波下游重钉（W4 scrim 语义移动字节，全部经通道 EXACT 背书）：
  worktrees 金帧（重导后与库内字节相同，仅 w80/w160 ansi 一度脏写后还原）、
  emission digest 0x5827fddc9f5aad50 → **0x78409566ff2310e8**、
  dialog/sidebar scrim 单测断言（overlay_backdrop / overlay_scrim）。

## 3. 终局验证

- **通道**：连跑 3 次 exit 0，报告（TUI-FULLSCREEN.md）sha256 三份相同
  （a78d1d95… 与终版一致）。
- **oc-tui 套件**：`cargo nextest run -p oc-tui` **1657/1657 通过**
  （3 skipped 为 env 门控；一次运行 e1_cells_paste_8k flaky 重试过，白名单
  项，终跑零 flaky）。
- **workspace**：`cargo fmt --check` clean；`cargo clippy --workspace
  --all-targets -- -D warnings` 0 error；`bash tools/guards/run.sh` all
  green。

## 4. 登记册终态

- GAPS-AND-PITFALLS 第三十一轮条目 → T9 终态（40/40 EXACT、无新 G-ID、
  W1-W4 四波摘要、golden 事项收口）。
- INDEX.md：通道 8 行 EXACT 40 / 未裁决 0；通道 1-7 总计刷新到第三十轮
  收口口径 **717 MATCH / 71 DIVERGE / 2 ADJUDICATED**（分通道行同步：
  config 77、ACP 111、SSE 2、JSON 156、compaction 60、HTTP 47、
  recordings 264/264）。
- TUI-FULLSCREEN.md 由通道 `--write` 终态化。

## 5. commits

- `db829a4a` fix(tui): dialog/palette family parity（scrim 语义/palette
  suggested/Status 树/Open placeholder+driver project seam/Sessions chrome）
- `d999e986` test(tui): golden 重导（3 批准项 + surf1 按 ref oracle）
- `c0bfa09c` test+docs: 下游重钉（scrim 断言/digest/f15）+ 登记终态

## 6. concerns

- `session_list_all_projects()` 目前恒 false（storage `all_projects` 有
  字段无消费者；ref 的 useStorage 持久化切换为有界缺口，已注释登记）。
- ref 引擎 whitespace-under-scrim 的机制层解释（为何无对话框时空格保留
  bold、有对话框时剥离）未深挖到 opentui 渲染器源码级——经验规则有 10
  网格 + 金帧两代形态旁证，如后续引擎升级出现新形态，以网格为准重验。
- diff driver 新增 ProjectList 臂与 app.rs live-loop 逐行镜像的纪律保持
  （注释标 app.rs:9834-9837）。

## 7. 评审落地（5 项发现，2026-11-14）

评审通过 + 5 项登记/注释级发现全部处置：

1. **connect-mid-turn footer 瞬态差登记**（Important）：GAPS 第三十一轮终态条目内
   新增「T9 文档化登记」节——ref live-append（rows.ts:219-227 appendFooter 尾部 +
   :306-308 removeFooter 迁移）vs rust reduce-inline 不可达产物；被
   w3_normalize.py §2→§4 耦合屏蔽（通道 40/40 不覆盖该瞬态形状 = 差分面已知
   收窄，非生产缺陷）；触发条件=连接落到进行中会话。不立新 G-ID。
2. **session_list_all_projects() 恒 false**：同节点名（storage 字段无消费者，
   ref 持久化切换为有界缺口）。
3. **INDEX.md 旧行矛盾**：「101 项分歧…G-412..G-422」行改 HTML 注释脚注
   （保留历史备查，不再与 717/71/2 总计并排呈现）。
4. **session.list suggested 静态近似**：command_palette.rs 对应臂补引证注释
   （session_history 静态快照 vs ref reactive data.session.list()，滞后一次
   resync，无 corpus case 可区分）。
5. scrim whitespace 经验规则：已登记，无动作。

验证：tui_fullscreen_selftest 25/25 OK；guards all green；git diff 仅涉
command_palette.rs / GAPS-AND-PITFALLS.md / INDEX.md / progress.md / 本报告。
