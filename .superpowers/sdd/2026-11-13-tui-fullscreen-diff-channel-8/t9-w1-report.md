# T9 W1 报告 — F1 footer 组件集差修复

状态：**完成**（F1 落地，通道 40→36 未裁决，4 case 翻 exact；含 golden 重钉清单待控制者裁决）。

## 修复内容（每个决策引 ref 源码）

F1 三件差，逐一按 ref 生产语义修复：

### 1. rust 多渲染 "Build" pill（40/40）→ 移除合成默认 agent

- 根因：`crates/oc-tui/src/components/session.rs` `composer_metadata()`
  在 agent 列表为空/未加载时 `.or_else(|| Some("build"))` 合成默认 agent。
- ref 语义：`local.agent.current()` =
  `agents().find(id===selected) ?? agents().at(0)`
  （`packages/tui/src/context/local.tsx:84-95`）——列表为空时
  `current()` 为 **undefined** → `agentLabel` undefined
  （`component/prompt/index.tsx:1550`）→ `PromptMetadataRow` 外层
  `Show when={(shell || agent) && (agent || model)}` 不成立 → **空白行**
  fallback（`component/prompt/metadata.tsx:46-48` 的 `<box height={1}/>`）。
  rust 渲染侧的等价 gate 已存在（`composer_solid.rs:390-397`），缺的只是
  数据源不再合成 "build"。
- 修复：删除 `.or_else(|| Some("build"))`；agent = selection 或列表首个
  非 hidden（无则空标签 → 行空白，与 ref 一致）。

### 2. rust 缺 cwd 面包屑（40/40）→ driver 补 boot LocationVcs

- 根因：不是 rust app 缺件——live loop 本就有
  `LoopRequest::LocationVcs`（app.rs:8703-8707 boot 发一次）+
  `LoopFetch::LocationVcs → set_footer_location`（app.rs:9908-9921）。
  差分 driver（tests/tui_fullscreen_diff_driver.rs）自行拼装 seam、
  未复刻 boot 块，且 pump `_ => {}` 丢弃 LocationVcs 结果。
- ref 语义：`locationLabel = abbreviateHome(directory) + ":" + branch`
  （`component/prompt/index.tsx:1605-1619`，branch 来自
  `data.location.vcs.info`）；ref harness census 证明 boot 走了
  /api/location + /api/vcs。
- 修复（driver 保真，非改生产渲染）：driver 以场景 `/api/location`
  stub 的 `directory`（fallback 任意 stub 的 `location.directory`）发
  `LoopRequest::LocationVcs`，pump drain 按 app.rs:9908-9928 原样应用
  （含 Err 静默）。

### 3. 右角版本号 ref="test" vs rust="2.0.24"（home 族）→ app 版本注入 seam

- **诊断勘误（与 T9 诊断 F1 节冲突，以 ref 源码为准）**：ref 右角
  "test" **不是 git 分支**，而是 ref 测试 harness 固定注入的
  `TuiApp { name:"test", version:"test", channel:"test" }`
  （`packages/tui/test/fixture/app.ts:35`）。生产语义 = 右角渲染
  boot 注入的 `context.app.version`
  （`feature-plugins/home/footer.tsx:99`，来源 runtime.tsx TuiAppProvider；
  生产 CLI 注入构建版本）。
- 修复：store 新增 `app_version` 字段（默认 `CARGO_PKG_VERSION`）+
  `app_version()/set_app_version()`（store.rs），`render_home` 改读
  store（home.rs）；driver 注入 `"test"` 复刻 ref fixture。

## 通道重跑结果

`python3 tools/diff/tui_fullscreen.py --write docs/audit/diff/TUI-FULLSCREEN.md`
→ gate exit 2（仍有未裁决），**40 → 36 未裁决**，EXACT 4：
home-empty、home-empty@120x30、fetch-503-model、fetch-503-model@120x30
（F1 独占 4 case 全部翻绿，与诊断 W1 预期一致）。
日志：/tmp/opencode/logs/t9-w1.log（EXIT=2）。

## Golden 安全阀触发 — 需重钉清单（stop-and-escalate，未自行重钉）

composer_metadata 移除合成 "build" 使空 agent 列表下的帧不再含
"Build" pill（rust 向 ref 修正的失配）。nextest -p oc-tui 全量：
1653/1657 过，**4 失配**，均为该修正的连带：

| # | 测试 | 需重钉物 |
|---|---|---|
| 1 | `opentui_emit_diff_tests::replay_corpus_emission_is_pinned` | 内联 digest `0xc663_d48d_7af3_8a77`（crates/oc-tui/src/opentui_emit_diff_tests.rs:139） |
| 2 | `screen_golden_tests::session_error_part_matches_text_grid_and_ansi` | tools/parity/goldens/tui-frames/session-error-part.w80.txt + .ansi |
| 3 | `screen_golden_tests::session_sidebar_live_matches_text_grid_and_ansi` | tools/parity/goldens/tui-frames/session-sidebar-live.w80/.w160 的 .txt + .ansi（4 文件） |
| 4 | `oc_tui::tree_scenarios scenarios_match_committed_captures` | tools/parity/tree-scenarios/*.json（已证实 tree-home.json：仅差被删的 "Build" text 节点；其余 scenario 在 regen 时一并确认）——重生成走 tools/parity/tree-scenario-capture.sh |

注：#2/#3 的失配未逐字节 diff（断言为 text_grid/ANSI 双比对）；从
tree-home 证据看差异即 "Build" 行消失，属预期修正。等待控制者裁决
（PARITY_UPDATE=1 / 手工重钉 / 重生成）。

## 验证

- nextest -p oc-tui --no-fail-fast：1653 passed / 4 failed（上表，全为
  golden 失配清单项）/ 3 skipped。
- 通道重跑见上（exit 2 = 36 未裁决，符合 W1 预期）。
- fmt/clippy：`cargo fmt --check && cargo clippy -p oc-tui --all-targets
  -- -D warnings` 双绿（t9-w1-lint.log EXIT=0）；重构后 driver 重跑
  DRVEXIT=0，4 个 exact case 双侧网格逐字符 0 差复核。

## Concerns

- /tmp/opencode/logs 曾被 root tee 污染属主导致 python append 失败，
  已 chown eternal130；后续 tee 日志请用非 gate 会话外的路径或注意属主。
- 通道 python **不可**包在 dev-gate 里跑（其内部对 rust driver 再调
  dev-gate，flock 自锁 → 2400s 超时假象）；直接 `python3 tools/diff/tui_fullscreen.py`。
- ref 右角 "test" 勘误已记录（§3）：T9 诊断的 "git 分支 test" 说法
  不成立，正确归因是 harness 注入版本。
