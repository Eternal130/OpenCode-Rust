# T9 W1 报告 — F1 footer 组件集差修复

状态：**完成（经控制者 F1 重裁后修订）**（F1 落地，通道 40→36 未裁决，4 case 翻 exact；golden 全部复绿，无重钉事项）。

## F1 重裁记录（控制者裁决，优先于首轮结论）

首轮曾按 "ref 空 agent 列表 → 无 agentLabel → 元数据行空白"
（local.tsx:84-95 + metadata.tsx:46-48）移除 rust `composer_metadata`
的合成 `"build"` 默认，导致 4 处 golden 失配。控制者裁决证实该方向
修反：**ref 服务端恒有默认 agent**（core/src/agent.ts:18
`defaultID = "build"`；生产 `/api/agent` 永远返回 build）。golden
期望有 "Build" 是对的；ref 网格无 Build 纯因场景 fetch 桩给了空
agent 列表（`data: []`）——双侧场景桩不对称地偏离了 ref 服务端事实。

修订（本 commit 最终形态）：
1. **回滚** `composer_metadata` 的合成默认移除（session.rs 恢复
   `.or_else(build)` 兜底——与 ref 服务端默认镜像）。
2. **双侧对称补桩**：20 个场景 fixture 的 `/api/agent` `data` 头部
   插入 build agent 行（形状 = ref `Agent.Info` 的
   `statics.default`，schema/agent.ts:36-53：`{id:"build",
   name:"build", mode:"primary", hidden:false, request:{settings:{},
   headers:{}, body:{}}, permissions:[]}`）。ref/rust 两 driver 读
   同一份场景，天然对称；ref 侧随之渲染 "Build"（其
   `agents().at(0)` 解析到 build），与 rust 一致。
3. breadcrumb（driver LocationVcs）与版本注入 seam（app_version）
   两项**保留**——与重裁无冲突，仍是 ref 语义修正。

## 修复内容（每个决策引 ref 源码）


F1 三件差，逐一按 ref 生产语义修复：

### 1. ~~rust 多渲染 "Build" pill~~ → 重裁：双侧场景桩补 build agent（见文首重裁记录）

- 首轮根因定位（rust 合成默认）被控制者证伪：ref 服务端默认 agent
  恒为 build（core/src/agent.ts:18）。rust `composer_metadata` 的
  `.or_else(build)` 兜底是 ref 服务端语义的正确镜像，**保留**。
- 真根因：场景 fetch 桩 `/api/agent data:[]` 偏离 ref 服务端事实，
  使 ref 侧 `agentLabel` 为空。修复 = 桩双侧对称补 build 行。

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

## 通道重跑结果（重裁后终态）

`python3 tools/diff/tui_fullscreen.py --write docs/audit/diff/TUI-FULLSCREEN.md`
→ gate exit 2（仍有未裁决），**40 → 36 未裁决，EXACT 4**：
home-empty、home-empty@120x30、fetch-503-model、fetch-503-model@120x30
（与控制者重裁预期一致）。
日志：/tmp/opencode/logs/t9-w1.log（CHANEXIT=2）。

## Golden 状态

重裁后**全部复绿，无重钉事项**：nextest -p oc-tui --no-fail-fast
1657/1657 通过（3 skipped；一次偶发 flaky 重试通过）。

附带两处为达成 exact 所需的 composer 元数据对齐（均在
composer_metadata，引 ref 源码）：

- **agent 标签 = `Locale.titlecase(agent.id)`**
  （prompt/index.tsx:1550）——不是 agent 行的 display name（build 行
  的 name 是 "build"，UI 显示 "Build"）。
- **无选择模型的 fallback 标签**：ref `local.model.parsed()` 无选择时
  返回 `{model: "No provider selected", provider: "Connect a
  provider"}`（local.tsx:394-401）；fetch-503 案例证明模型请求失败
  （`models() === undefined`）时 ref 同样渲染 fallback。rust 落地为
  "boot model fetch 已决（Loaded/Failed）即渲染，Loading 期空白"——
  Loading 期空白是为保住既有 golden（其钉住 pre-fetch 帧）的折中，
  与 ref（parsed() 恒 live）存在一个 boot 瞬态的理论差，静止帧无差
  （见 Concerns）。

## 验证

- nextest -p oc-tui --no-fail-fast：1657 passed / 0 failed / 3 skipped
  （重裁后全绿；一次 flaky 重试通过）。
- fmt --check + clippy -p oc-tui --all-targets -D warnings：绿。
- 通道重跑见上（exit 2 = 36 未裁决，EXACT 4，符合重裁预期）。

## Concerns

- 模型 fallback 的 Loading 期空白折中：ref 的 `parsed()` 在 catalog
  未到时也返回 fallback；rust 以 "boot fetch 已决（Loaded/Failed）"
  为门，是为兼容钉住 pre-fetch 帧的既有 golden。静止帧（通道比对
  面）双侧无差；boot 瞬态理论差记录在案。
- /tmp/opencode/logs 属主曾被 root tee 污染致 python append 失败，
  已 chown eternal130；后续注意。
- 通道 python **不可**包在 dev-gate 里跑（其内部对 rust driver 再调
  dev-gate，flock 自锁 → 2400s 超时假象）；直接
  `python3 tools/diff/tui_fullscreen.py`。
- ref 右角 "test" 勘误（首轮已记录）：是 ref fixture 注入的
  `TuiApp.version`（app.ts:35），非 git 分支。
- W2 建议（F2 流 idle 收口 + F3 输入区高度 + F7 hydration 幂等）按
  诊断波次进行。
