# T9 W3 — T-ord 转录行序族 + leader+l 和弦 + turn 时长（通道 8）

基线 d1726585（W2 后：10 exact / 30 diverge）→ W3 后：**28 exact / 12 diverge**
（`python3 tools/diff/tui_fullscreen.py`，exit 2 = 剩余 12 未裁决，全部落在 W4 的
dialog/palette 风格族）。

## 1. T-ord 族（16 case → 16 exact）

受影响 case（两个尺寸档各 8 场景）：session-markdown / session-multi-turn /
session-reasoning / session-scroll / session-step-failed / session-text /
session-tool-success / session-wrap。**全部转 exact。**

### 根因（三层，全部是场景/harness 数据违反生产不变量，而非 rust 渲染语义错位）

对两侧做 60 行高全量转录 dump（自建 debug manifest），把行序差分解到消息流：

1. **事件 id 重复（rust 侧丢事件）**。multi-turn 场景对 3 个 turn 复用
   `evt_step_started` 等 id。生产总线 id 唯一；两侧客户端都按 id 去重重放
   （ref：SSE 重连 replay 过滤；rust：`store.rs is_replayed`，F2 契约移植）。
   ref fixture 把事件直接注入 app 事件流、绕过了自己的去重层，所以只有 rust
   丢了第 2/3 个 turn 的 step/text 事件 → 只有 msg_asst_1 拿到 footer。
2. **wire 消息缺 `finish`/`time.completed`（ref footer 位置漂移）**。生产
   `message.list` 对已完成 turn 一定带 `finish`（client types
   `SessionMessageAssistant.finish?`；`reduceSessionRows` 仅在 message.finish
   时 INLINE 发 footer 行，rows.ts:403-406）。fixture 的消息 stub 不带，
   ref 水合后没有 inline footer；随后重放的 `session.step.ended` 走 LIVE 路
   `appendFooter`，追加到**转录尾部**（rows.ts:219-227），且
   `session.step.started` 先 `removeFooter` 删掉水合 footer（rows.ts:306-308）
   ——60 行 dump 证实：事件前 ref 是 inline footer（[A3,f13,Q3,…]），事件后
   footer 全部迁移到尾部（[A3,Q3,…,Q1,f4,f8,f13]）。rust 是 reduce 架构、
   footer 永远 inline，该“迁移”产物不可达也无需移植（生产不会对已持久化
   turn 重放 step 生命周期）。
3. **session-reasoning 的 text 事件 ordinal=1 与 wire 不符**（wire 只有一个
   text part，序号 0）：ref `appendPart(text:1)` `hasPart` 不命中，在尾部追加
   了重复文本行。

### 修复（场景归一化 + rust 侧两个真 bug）

- **`tools/diff/tui/w3_normalize.py`**（幂等，入库）：① 事件 id 去重
  （重复 id 追加 `_N`）；② 从每个 assistant 的终态事件
  （step.ended/step.failed）回填 wire 的 `finish`/`time.completed`
  （failed 再回填 `error`）；③ text 事件 ordinal 夹到 wire part 序号；
  ④ 剪除“对已持久化 turn 重放的 step/text/reasoning/tool 生命周期事件”
  （wire 已带 finish 的 assistant 即视为持久化 turn）。改写 10 个场景文件。
- **rust `store.rs` step.started**：ref data.ts:843 会把
  `time.created = event.data.started` 重置；rust 之前不动 created，导致
  footer 时长回落基准错（见 §3）。
- **rust `tool_detail.rs` shell_block**：① 补 `BlockTool` 的 error 行
  （index.tsx:2700-2702+2767-2769：失败 shell 工具在块内追加
  `state.error.message`，error 色，块变体无 errorExpanded 门控）；
  ② rail 颜色修正：`┃` 落在 raised 填充上（fg=background.base、
  bg=background.raised；ref session-tool-failed 捕获 + BlockTool
  backgroundColor=raised，状态无关）。session-tool-failed 两档转 exact。
- **rust `session.rs` 滚动 viewport 差一**：F3 给 transcript 预算加了列底
  pad `+1` 之后，`metrics.viewport = transcript_height - 1` 把 jump slot 行
  重复扣了一次（24 行屏 viewport 14，ref 为 15——“long-reply corpus pins
  viewport 15” 的注释即 F3 之前写的）。粘底窗口整体高一行，所有需滚动的
  转录（multi-turn 小档、scroll 全档）错位。改为 `viewport =
  transcript_height`。session-scroll（含 pagedown）两档随之转 exact。

## 2. leader+l 和弦裁定（ref fixture harness 编码缺陷 → 修 ref driver，rust 不动）

- **ref 生产语义**：leader 默认 `ctrl+x`（`config/keybind.ts:41
  LeaderDefault`），经 kitty 键匹配器匹配（`context/keymap.tsx:190
  createKeyMatcher`，key+modifier）。kitty 协议下裸 `\x18`（CAN）被解析为
  codepoint 24、**不带 ctrl 修饰**——ref driver 自己的 ctrl token 注释
  （tui_fullscreen_ref.ts）就写明了这一点，却把 `leader` 映成裸 `\x18`。
- **结论**：生产 ref 不把裸 `\x18` 识别为 leader；W2 看到的“ref 对话框
  关闭、'l' 落入 prompt”是 ref driver 的 leader 编码错误（与它自己的
  ctrl 编码约定不一致），rust 侧（driver 按 `ctrl+x` 编码、app 识别为
  leader）才是生产语义。**非 rust 超集，rust 不收紧。**
- **修复**：`tools/diff/drivers/tui_fullscreen_ref.ts` 的 `pressToken` 对
  `leader` 走 kitty 原生 `pressKey("x", { ctrl: true })`。session-tabs 的
  ref 腿随后与 rust 一致打开 Sessions 对话框（该 case 剩余分歧全部落入
  dialog 风格族，见 §4）。

## 3. turn 时长 5ms vs 3ms（= rust 漏掉 step.started 的 created 重置）

时长 = `turnDuration`（rows.ts:430-440）= `completed − input.created`，无
前序 input 时回落 `message.time.created`。ref 的 data.ts:843 在
step.started 把 `time.created = event.data.started`（fixture started=0），
回落基准变 0 → wrap 5ms/reasoning 7ms；rust 保留水合 created=2 → 3ms/5ms。
rust `store.rs` step.started 分支补上该重置后一致（事件被 §1-④ 剪除的
case 走 wire completed，两侧同源）。属“渲染语义差”（rust 漏移植），已修。

## 4. 通道计数与剩余项

- 40 case：**exact 28 / diverge 12 / structural 0**（30→12）。
- 剩余 12 = dialog/palette 风格族（W4）：command-palette、dialog-open-recent、
  dialog-status、models-dialog、session-tabs（leader 修复后只剩 dialog
  chrome 风格差：标题 bold、bg 30/12 量化、布局缩进）、sidebar-toggle。
- session-stream-idle / home-* / fetch-503 等原 exact case 全部保持 exact
  （归一化未触及：无终态事件的 assistant 不回填、事件不剪）。

## 5. golden 事项（goldens 未动）

- **既有红（HEAD=d1726585 即红，与本波无关，隔离 worktree 复核）**：
  `screen_golden_tests::{session_error_part, session_slash_palette,
  session_sidebar_live}` —— 三个 golden 的 composer 挂点在 W2 的 mount
  几何变化后低了 1 行（golden rails 19-21，实跑 18-20），W2 未刷新。
- **新失配（本波引入，列清单不回写 golden）**：
  `screen_golden_tests::surf1_detail_states`（ANSI 首分歧 byte 60）——该
  golden 是**本地 fixture 形状 pin**（“not in the reference corpus”，测试
  注释自述），pin 了旧 shell 块 rail 的 `bg=background`；ref oracle 捕获
  （session-tool-failed）+ BlockTool 源码证明 rail 恒为 `bg=raised`。按
  oracle 修正渲染后该本地 pin 过期。建议 W4 用 ref 捕获重制该 pin。
- `cargo nextest -p oc-tui`：1657 中 1653 过，4 失败 = 上述 3 既有红 +
  1 新失配；clippy -D warnings clean；fmt clean。

## 6. commits

见 git log（英文 commit）：场景归一化+normalizer+ref driver leader 编码；
rust 侧三处修复（step.started created / shell_block error 行+rail /
scroll viewport）；本报告 + 通道重跑的 TUI-FULLSCREEN.md。
