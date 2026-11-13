# 差分通道 8（TUI 整屏差分·发版回归门）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 同一场景脚本双侧（ref 真 TUI / rust 真渲染链）无头渲染静息整屏，cell 级差分进 CAUSES 裁决体系，成为第 22 条幂等扫描命令与发版门（exit 0 绿 / 2 未裁决红 / 1 infra）。

**Architecture:** 通道 7 模板（python 编排器 + bun ref driver + env-gated rust nextest driver）。ref 腿复用 ref 自身无头测试设施（createTestRenderer/createAppFixture 模式，照抄 tools/parity/tui-dump 的 staging/import 机制）；rust 腿 in-process 场景重放（事件摄取→store→press→定点排空→`App::frame()`）。比对 = 稠密 cell 网格规范 JSON（char+规范属性+RGB），EXACT 主判、STRUCTURAL(±8) 裁决辅助。

**Tech Stack:** Python3（编排器）、Bun 1.4.2 + ref 树 pinned 2.0.24（ref driver）、Rust oc-tui nextest（rust driver）、JSON corpus。

**Spec:** `docs/superpowers/specs/2026-11-13-tui-fullscreen-diff-channel-8-design.md`（执行者必读——本计划只钉接口与验证，论证在 spec）。

## Global Constraints

- cargo 一律经 `tools/dev-gate.sh '...'`；nextest 是唯一测试 harness；测试日志 tee 到 `/tmp/opencode/logs/`
- commits 用英语；身份 Eternal130（禁止 --author）
- ref 树只读（/opt/opencode-ref）；bun 钉 1.4.2
- rust driver env 门控：`TUI_DIFF_MANIFEST` / `TUI_DIFF_OUT`；env 不设 = passing no-op（nextest 常绿）
- ref driver：env `DIFF_REF_ROOT`，argv `--manifest <file> --out <file>`（对齐通道 7 ref driver 约定）
- 退出码：0 = 门绿（未裁决=0）；2 = 门红（存在未裁决分歧，报告仍写盘）；1 = infra 失败
- STAGE 目录 `/tmp/opencode/diff-tuifull`；corpus 在 `tools/diff/corpus/tui/scenarios/`
- 常量：定点排空 K=3 连续同帧、排空上限 64；ref `waitForVisualIdle({quietFrames:3, maxFrames:600})`
- 主题钉死 `opencode` v2-dark；尺寸 80×24 与 120×30
- 全程 fmt/clippy/`bash tools/guards/run.sh` 不得回红（新文件不得触发 process_safety 等）
- 规范网格 schema `"tui-diff-grid/1"`、场景 schema `"tui-diff-scenario/1"`（下述 Task 1/2 钉死字段）

## Review Focus

1. **ref spans 尾部空白缺省 vs rust 稠密网格**——短行场景下 first_diff 假阳性。测试：Task 5 单测（短行合成网格，规范侧补空格后相等）。
2. **属性词汇表集外位**——映射表漏项被静默丢弃 = 假绿。测试：Task 5 单测（合成 unknown 属性位必须计分歧，输出 `unknown:<bit>`）。
3. **Default 色哨兵不对称**——一侧解析成 RGB 一侧留哨兵。测试：Task 5 单测（哨兵 vs RGB = 分歧；双侧哨兵 = 相等）。
4. **ref 腿非确定性泄漏**（时钟/相对时间/请求顺序）。测试：Task 2/7 每场景双跑字节相同（tui-dump `--verify` 模式先例）。
5. **fetch 桩端点缺口**——ref 请求了未桩端点 → 503 错误面板被静默当场景比对。测试：Task 2 driver 记录请求普查，未命中桩的请求 ≠ 空集时该场景输出 `stub_gap` 标记，Task 4 编排器将其列为 infra 级错误（exit 1 路径单测）。

---

### Task 1: 场景 schema + 3 个 smoke 场景

**Files:**
- Create: `tools/diff/corpus/tui/scenarios/home-empty.json`、`session-text.json`、`session-markdown.json`
- Create: `tools/diff/corpus/tui/SCHEMA.md`（schema 文档 + 字段语义，双端实现者共读）

**Interfaces:**
- Produces: 场景 schema `"tui-diff-scenario/1"`：
```json
{"schema":"tui-diff-scenario/1","id":"home-empty","width":80,"height":24,
 "fetch":{"/api/path":{"status":200,"body":<json>}},
 "events":[<ref /api/event SSE payload 形状的 JSON 数组>],
 "keys":["<tui_diff.rs ScriptStep 键名>"],
 "settle_ms":200,"note":"一句话场景语义，双侧实现等价性论证处"}
```
- 键名词汇：照抄 `tools/parity/src/tui_diff.rs` 的 ScriptStep 双端键名表（先 grep 确认其 rust 侧映射函数名并写进 SCHEMA.md）。

- [ ] **Step 1: 探测 ref TUI 启动期请求全集**：临时 bun 脚本（用 ref `packages/tui/test/fixture/app.ts` 的 createAppFixture 起 app 不喂桩外的键），打印全部请求 path。产出写进 SCHEMA.md「端点普查」节。
- [ ] **Step 2: 写 3 个场景**（home-empty：空会话列表+无事件；session-text：一条 user+assistant 纯文本消息；session-markdown：assistant 消息含标题/列表/代码块）。fetch 桩覆盖 Step 1 普查到的全部端点；事件用 ref `/api/event` SSE payload 形状。
- [ ] **Step 3: 校验**：`python3 -c "import json,glob;[json.load(open(p)) for p in glob.glob('tools/diff/corpus/tui/scenarios/*.json')]"` 通过；SCHEMA.md 含端点普查与键名表。
- [ ] **Step 4: Commit** `feat(diff-tui): channel-8 scenario schema + smoke corpus`

### Task 2: ref bun driver

**Files:**
- Create: `tools/diff/drivers/tui_fullscreen_ref.ts`

**Interfaces:**
- Consumes: Task 1 场景文件；ref 树 `packages/tui/test/fixture/app.ts`（createAppFixture——优先直接 import；其内部依赖解析不通时，退而在 driver 内复刻 ~50 行 fixture 装配，import `@opentui/core/testing`，二者取其一并在文件头注释声明）
- Produces: `--out` JSON：`{"schema":"tui-diff-grid/1","id":...,"width":W,"height":H,"cells":[[{"ch":"a","attrs":["bold"],"fg":[r,g,b],"bg":[r,g,b]}]]}`——稠密 rows×cols（尾部空白显式空格 cell）；attrs 为规范集 `["bold","dim","italic","underline","strikethrough","reverse"]` 的排序子集，集外位输出 `"unknown:<n>"`；fg/bg 恒为 0-255 RGB（终端默认色解析为 opencode v2-dark 主题默认前景/背景 RGB，解析不了输出 `[-1,-1,-1]` 哨兵）；附 `"stub_gaps":[<未命中桩的请求 path>]` 与 `"request_census":{path:count}`。

- [ ] **Step 1: staging 机制照抄 `tools/parity/tui-dump/`**（driver 如何被拷进 STAGE、node_modules symlink、`DIFF_REF_ROOT` 注入、bun 1.4.2 调用）——读该目录并在文件头注释映射关系。
- [ ] **Step 2: 实现 driver 主体**：逐场景（manifest 排序）createAppFixture({width,height, config:{animations:false}})（spec §5 时钟冻结纪律；时间显示类字段由场景 fetch 桩注入固定时间戳）→ fetch 桩按场景 `fetch` 表 + 未命中 path 记入 stub_gaps → `events` 逐个 `events.emit` → `keys` 逐个 `mockInput.pressKey` → `await setup.waitForVisualIdle({quietFrames:3,maxFrames:600})` → `captureSpans()` → 栅格化+属性映射+Default→主题 RGB → 规范 JSON。
- [ ] **Step 3: 双跑确定性验证**：同 manifest 连跑两次，两份 out 字节相同（`cmp`）。Expected: identical。
- [ ] **Step 4: smoke 场景产出验证**：3 场景 ×2 尺寸网格 JSON `python3 -m json.tool` 全部合法、stub_gaps 全空。
- [ ] **Step 5: Commit** `feat(diff-tui): ref headless driver over createAppFixture`

### Task 3: rust env-gated driver

**Files:**
- Create: `crates/oc-tui/tests/tui_fullscreen_diff_driver.rs`

**Interfaces:**
- Consumes: Task 1 场景；`oc_tui::app::App`（`App::new((w,h))`、`app.press(key)`、`app.frame()->render::Frame`）；摄取 seam（Step 1 spike 决定）
- Produces: 与 Task 2 逐字段一致的 `"tui-diff-grid/1"` JSON（`TUI_DIFF_OUT` 路径；attrs 从 `render::Cell.attrs: u16` ECMA-48 位映射规范集；`CellColor::Default` → 主题默认 RGB，同款哨兵规则）

- [ ] **Step 1: spike——定摄取 seam**：查 `app.rs` 活循环（`run_with_size` :8125 起）里 SSE 事件→store 的应用函数；三选一（spec §9 风险 5 优先序）：(a) 该应用路径可直接调用 → 场景 events 直接喂它；(b) 只能在活循环内 → 用 `tests/vt_harness.rs` 的 FakeLlm 起桩 + oc_client 真摄取 + 手动泵事件循环（无渲染发射）；(c) 个别状态难经 server 响应构造 → 场景加 `store_ops` 旁路字段直写 store（场景 `note` 论证双侧语义等价）。时钟依赖 store 测试态冻结 0（不调用 advance_clock）。决定与 file:line 写进文件头注释。**渲染链必须经 `app.frame()`，不得绕过。**
- [ ] **Step 2: no-op 门测试先行**：无 env 时 nextest 见 passing no-op（照抄 `provider_recordings_diff_driver.rs:170-175` 模式）。
- [ ] **Step 3: 实现**：manifest 逐场景（排序）→ 摄取 → keys（`press`，键名经 SCHEMA.md 表映射）→ 定点排空（`serialize_canonical(frame)` 连续 3 次相同，上限 64，超限 = 该场景错误记录）→ `frame()` → 规范 JSON。
- [ ] **Step 4: 验证**：`TUI_DIFF_MANIFEST=... TUI_DIFF_OUT=... tools/dev-gate.sh 'cargo nextest run -p oc-tui --test tui_fullscreen_diff_driver'` 出网格；同 manifest 双跑 `cmp` 字节相同；无 env 时全绿 no-op。
- [ ] **Step 5: fmt/clippy/guards 三绿 + Commit** `feat(diff-tui): rust in-process scenario replay driver`

### Task 4: 编排器——manifest、双侧调度、infra 语义

**Files:**
- Create: `tools/diff/tui_fullscreen.py`

**Interfaces:**
- Consumes: Task 2/3 drivers（staging 照通道 7 `run_ref` :247-261：driver 拷贝 + node_modules symlink + `DIFF_REF_ROOT`；rust 腿 nextest + env 注入）
- Produces: `run(side) -> dict[id, grid]`；argparse `--ref/--write/--manifest-only`；infra 失败（驱动 rc≠0 / stub_gaps 非空 / 场景缺一侧结果）→ `SystemExit(1)`

- [ ] **Step 1: docstring 规格书**（照通道 7 :1-43：corpus 来源、双侧驱动、归一化声明、范围外、输出、exit 0/1/2 语义）。
- [ ] **Step 2: build_manifest + run_ref + run_rust + STAGE 清理**（通道 7 骨架映射，sorted 保确定性）。
- [ ] **Step 3: 验证**：`--manifest-only` 出排序 manifest；smoke 场景端到端双侧出网格；故意断一个桩端点 → exit 1。
- [ ] **Step 4: Commit** `feat(diff-tui): channel-8 orchestrator skeleton (dual dispatch, infra semantics)`

### Task 5: 编排器——归一化、比对、判门

**Files:**
- Modify: `tools/diff/tui_fullscreen.py`
- Test: `tools/diff/tui_fullscreen_selftest.py`（`python3 -m unittest` 可独立运行）

**Interfaces:**
- Produces: `normalize(grid)`（时间字段剥换钩子）、`compare(ref_grid, rust_grid) -> {verdict: "exact"|"structural"|"diverge", first_diff: {row,col,field,ref,rust}, cells_diff_count}`、`verdict(results, causes) -> exit_code`
- STRUCTURAL = 仅 RGB 分量差且每通道 ≤8、char/attrs 全等（G-368 容差；**仍是分歧进 CAUSES**）

- [ ] **Step 1: 先写失败单测**：五组合成网格——全等 / 单字符差（first_diff 定位 row,col,field）/ ±7 RGB（structural）/ unknown 属性位（diverge 且 attrs 里出现 `unknown:`）/ 哨兵 vs RGB（diverge）——断言 compare 输出。
- [ ] **Step 2: 跑单测确认失败** `python3 -m unittest tools.diff.tui_fullscreen_selftest -v`（或 `python3 tools/diff/tui_fullscreen_selftest.py`）。
- [ ] **Step 3: 实现 normalize/compare/verdict** 使单测过。
- [ ] **Step 4: smoke 端到端**：3 场景×2 尺寸出 verdict 表；人为改一个 cell 制造 diverge → exit 2；复原 → 记录当前基线数。
- [ ] **Step 5: Commit** `feat(diff-tui): grid normalization, exact/structural compare, gate verdict`

### Task 6: CAUSES 裁决表 + 六段报告

**Files:**
- Modify: `tools/diff/tui_fullscreen.py`
- Create: `docs/audit/diff/TUI-FULLSCREEN.md`（由 `--write` 生成）

**Interfaces:**
- Produces: `CAUSES: [(label, matcher(divergence)->bool, 双侧 file:line 判定文)]` 预填簇 G-28/G-332/G-239·G-331/G-368（matcher 按场景 id 前缀+分歧形状，lambda 模式照通道 7 :348-393）；`write_report` 六段：头部复现行+双侧驱动面 file:line → 归一化声明 → 总量/EXACT/STRUCTURAL/CAUSES/未裁决表 → 分歧明细（first_diff）→ CAUSES 定性 → 范围外声明（框架层 127 场景边界/动画豁免/活体 v2）

- [ ] **Step 1: CAUSES matcher 单测**（合成分歧记录命中/不命中各簇）。
- [ ] **Step 2: 实现 + `--write docs/audit/diff/TUI-FULLSCREEN.md` 生成**，报告含六段与四计数。
- [ ] **Step 3: Commit** `feat(diff-tui): causes adjudication table + report writer`

### Task 7: 语料扩到 ~20 场景×2 尺寸

**Files:**
- Create: `tools/diff/corpus/tui/scenarios/*.json`（新增 ~17 个）

**Interfaces:**
- Consumes: Task 1 SCHEMA.md；场景族：流式中途→静息、推理段、工具输出（成功/失败）、长文换行、tabs、sidebar、models 面板、对话框/命令面板、mini footer、**插件 panel 屏（G-28 首轮登记面）**、fetch 503 错误面板、宽窄两档专属（docked vs fullscreen panel）
- 每场景 `note` 写双侧实现等价性论证

- [ ] **Step 1: 批量写场景**（每写一批即双跑验证，避免最后一起调试）。
- [ ] **Step 2: 全量确定性门**：双侧各双跑字节相同 ×20 场景×2 尺寸。
- [ ] **Step 3: 全量跑通道**，基线分歧清单落盘。
- [ ] **Step 4: Commit** `feat(diff-tui): full scenario corpus (20x2) + first baseline`

### Task 8: 登记接入

**Files:**
- Modify: `docs/audit/diff/INDEX.md`（头 :1 "7 通道"→8、总表 :7-17 加行、总计段、语料来源 :36-39、**范围外 :41-43 收窄**——渲染层从整层豁免改为"通道 8 覆盖无头静息整屏；动画中间帧（G-128/G-49/G-368）与活体交互序列仍范围外"）
- Modify: `docs/GAPS-AND-PITFALLS.md:35/:3124` 口径句 diff 7→8 + 新增本轮条目（通道 8 立项、spec/plan 链接、首轮基线数字）

- [ ] **Step 1: 两文档五处修改**，grep 验证：`grep -c "diff 8" docs/GAPS-AND-PITFALLS.md` ≥1、INDEX 表含 `tui_fullscreen.py` 行。
- [ ] **Step 2: Commit** `docs(register): channel-8 admission (diff 8) — INDEX/GAPS integration`

### Task 9: 首轮裁决循环（绿灯即交付，spec §11.4）

**Files:**
- Modify: 按归因结论而定（rust 侧修复 或 `tools/diff/tui_fullscreen.py` CAUSES 扩充 或 GAPS 新 G-ID）
- Consumes: Task 7 基线分歧清单
- Produces: `python3 tools/diff/tui_fullscreen.py --write docs/audit/diff/TUI-FULLSCREEN.md` → **exit 0**

- [ ] **Step 1: 逐分歧归因**：CAUSES 已裁决 → pass；未裁决 → root-cause：bug 则修复（修后 `tools/dev-gate.sh 'cargo nextest run -p <受影响crate>'` + 通道重跑确认消失）；有据分歧则登记新 G-ID + CAUSES 条目（双侧 file:line 判定文）。**不可归因的疑难 = stop-and-escalate 交用户**（30 轮纪律）。
- [ ] **Step 2: 循环至 exit 0**：每轮把未裁决数写进测试日志（tee `/tmp/opencode/logs/tui-fullscreen-round<N>.log`）。
- [ ] **Step 3: 终态验证**：通道连跑 3 次幂等（报告字节相同、exit 0）；workspace fmt/clippy/guards 三绿；GAPS 本轮条目含最终四计数。
- [ ] **Step 4: Commit(s)** 按修复分组，终局 `docs(register): channel-8 green — first-round adjudication complete`，向用户汇报四计数与裁决摘要。
