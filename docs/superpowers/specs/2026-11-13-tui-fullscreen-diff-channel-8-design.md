# 差分通道 8：TUI 整屏差分（发版回归门）设计

日期：2026-11-13 · 状态：待用户批准 · 侦察基线：三路并行侦察（通道架构 / oc-tui 渲染能力 / ref TUI 可驱动性）

## 1. 意图与为什么

7 个既有差分通道覆盖协议/配置/HTTP/provider 语义层，TUI 渲染层目前在
`docs/audit/diff/INDEX.md:43` 整层声明范围外。beta.14 已发、人工测试在即，
需要机械化屏效回归门补上这块观测面。

**用户裁决（2026-11-13）**：通道 8 = 发版回归门——与其他 7 通道同权，
新屏差进登记册、拦后续发版；不是从宽的测量仪。

## 2. 目标

1. 同一场景脚本喂给双侧真实 TUI 代码，各自无头渲染至**静息帧**，
   比对整屏网格（char / attrs / fg / bg）。
2. 成为第 22 条幂等扫描命令：`python3 tools/diff/tui_fullscreen.py`。
3. 登记接入四处：新报告 `docs/audit/diff/TUI-FULLSCREEN.md`；
   INDEX.md（头计数、总表行、总计段、语料来源构成、**范围外声明收窄**：
   渲染层从"整层范围外"改为"通道 8 覆盖静息整屏，余下 X"）；
   `docs/GAPS-AND-PITFALLS.md:35/:3124` 口径句 diff 7→8。
4. 首轮跑出基线：预期分歧簇（G-28 有界插件渲染、G-332 有界 markdown
   行形状、G-239/G-331 panel 有界偏离）逐条裁决登记；**未解释分歧 =
   新 G-ID = 拦发版**。

## 3. 非目标（v1 明确不做）

- **活体双 PTY 交互差分**：`tools/parity/src/tui_diff.rs`（4000 行，
  EXACT/STRUCTURAL 双模式）与 p10_cross_tui 已存在，吸收进通道体系是
  v2 扩展，本期不做。通道 8 v1 只做无头静息帧。
- **动画/中间帧对拍**：G-128/G-49/G-368（100ms 泵 vs ref rAF·targetFps:60）
  维持文档化豁免。静息帧 + 双侧冻结时钟使泵不可见。
- **ci.yml 接入**：与其余 7 通道一致——本地证据电池（发版前跑）。
  ref 树 + bun + Zig 原生包不适合 CI runner。

## 4. 反目标（做了即失败）

- **golden 自证**（GAPS:305 纪律）：两侧都必须跑真实代码——ref 腿跑
  ref 真 TUI（经其自身 fixture harness），rust 腿跑真渲染链
  （oc_client 摄取 → store → `App::frame()`）。**不许用预录 golden
  冒充 ref 腿**（tui-dump 47 场景 golden 是既有 crate 内验收面，与
  本通道并存、不复用为腿）。
- **静默豁免**：范围外/降档必须逐项计数显式声明（通道 6/7 纪律）。
- **重钉无审批**：oracle/基准不符 = 重钉事件 = stop-and-escalate
  （GAPS:314）。

## 5. 约束

- ref 树只读（/opt/opencode-ref，pinned）；bun 钉 1.4.2（与
  runtime-gates BUN_PIN / release.yml pin 同源纪律）。
- 主题钉死 ref 默认 `"opencode"` v2-dark
  （`packages/tui/src/theme/assets/v2/opencode.json` = rust
  `theme.rs:114-192` corpus 同源 token 树）。
- 尺寸 v1 两档：80×24 与 120×30。
- 时钟冻结：rust `advance_clock(0)`（store 时钟测试态本就冻结 0）；
  ref 场景注入固定时间字段 + `config.animations:false` + ManualClock
  （如需）。
- rust driver env-gated no-op 纪律：`TUI_DIFF_MANIFEST/OUT` 命名
  （对齐 `ACP_DIFF_*`/`CONFIG_DIFF_*` 全仓约定），env 不设 = passing
  no-op，nextest 常绿。
- 退出码（三态，门语义机械化）：exit 0 = 报告且**未裁决分歧 = 0**
  （门绿）；exit 2 = 报告已落盘但存在未裁决分歧（**门红 = 拦发版**，
  CAUSES 已裁决簇命中 = pass-with-count 不拦）；exit 1 = 基础设施失败
  （boot 不可能/驱动崩溃）。报告恒写盘——门红也先出报告。
- cargo 一律经 dev-gate；nextest 为唯一测试 harness；测试日志 tee 落盘。

## 6. 结构（通道 7 模板逐件映射）

| # | 部件 | 位置 | 说明 |
|---|---|---|---|
| 1 | 场景语料 | `tools/diff/corpus/tui/scenarios/*.json` | 每场景 `{id, size, fetch_stubs, event_seq, keys, settle}`；排序遍历保确定性 |
| 2 | ref bun driver | `tools/diff/drivers/tui_fullscreen_ref.ts` | 复用 ref `packages/tui/test/fixture/app.ts` 的 `createAppFixture`（真 `@opencode/client` + 进程内 `Bun.serve` 桩 + `createEventStream` SSE 回放 + `mockInput` 键序）→ `waitForVisualIdle()` 静息 → `captureSpans()` 导出规范化 JSON 网格 |
| 3 | rust driver | `crates/oc-tui/tests/tui_fullscreen_diff_driver.rs` | env-gated：同脚本经 FakeLlm HTTP+SSE 桩 → 真 oc_client 摄取 → store → `press()` 键序（键名词汇**照抄 `tools/parity/src/tui_diff.rs` ScriptStep 的双端映射表**，不新造）→ 时钟 0 → `app.frame()` → `Frame.cells` → 同格式 JSON。两侧输出 schema 逐字段一致 |
| 4 | 编排器 | `tools/diff/tui_fullscreen.py` | 通道 7 骨架：docstring 规格书、sorted manifest、ref staging（driver 拷贝 + node_modules symlink + `DIFF_REF_ROOT`）、归一化、EXACT 比对、`first_diff`、CAUSES 裁决表、六段报告、exit 语义 |
| 5 | 报告+登记 | `docs/audit/diff/TUI-FULLSCREEN.md` 等 | 见目标 3 |

## 7. 场景语料 v1（约 20 场景 × 2 尺寸）

静息态覆盖：空启动/home；会话转录（纯文本/推理段/工具输出/markdown
变体/代码块/长文换行）；流式中途→静息；tabs/sidebar/models 面板；
对话框；mini 模式 footer；**插件 panel 屏（故意纳入 = G-28 首轮登记面）**；
错误面板（fetch 503 路径）。

语料来源 = 自造（FakeLlm 桩脚本改写）+ tui-dump 47 场景语义子集映射
（只借场景语义，不字节复用——两侧都重放真实代码）。

## 8. 比对语义与归一化

- **规范网格形式 = 稠密 cell 网格**：ref 的 spans（行程编码、尾部空白
  可缺省）在导出时栅格化为 rows×cols 稠密网格（尾部空白显式补为
  空格 cell）；rust `Frame.cells` 本就是稠密网格。`first_diff` 以
  cell 为粒度。
- **属性词汇表**：定义规范属性集 = bold / italic / underline /
  strikethrough / reverse / dim 的固定位序（枚举钉在编排器常量里），
  ref `attributes: number` 与 rust ECMA-48 `attrs: u16` 各自映射到
  规范位；**任一侧出现规范集之外的属性 = 记录为分歧**（不静默丢弃），
  防止映射表错漏制造假绿。
- **`CellColor::Default` 解析**：双侧都必须把"终端默认色"解析为钉死
  主题（opencode.json v2-dark）的默认前景/背景具体 RGB 后再比对；
  不可解析时输出独立哨兵值并计为分歧——不允许一侧 RGB 一侧哨兵的
  静默豁免。
- **主判 EXACT**：cell `{char, 规范attrs, fg RGB, bg RGB}` 序列化
  逐字节相等。真彩值经各自主题 token 解析，同源 opencode.json 保证
  映射一致。
- **降档 STRUCTURAL（裁决辅助，不是豁免）**：字形掩码 + RGB ±8/channel
  容差（G-368 颜色量化经验推导的既定容差）。降档命中仍是分歧，进
  CAUSES 归簇登记。
- **时间类字段**：fetch 桩注入固定时间戳；相对时间显示场景内在脚本里
  冻结；两侧时钟注入 0。
- **静息判定（双侧确定性）**：rust 腿无 visual-idle 探测，改用**定点
  排空**——键/事件应用后反复 `frame()`，直到输出连续 K 次字节相同
  （渲染在 store+时钟 0 下纯同步，定点必达），带上限防死循环；K 与
  上限为编排器常量。ref 腿钉死 `waitForVisualIdle({quietFrames: N,
  maxFrames: M})` 的 N/M 为同源常量（N=K 语义对齐）。场景内的
  `settle` 字段只声明前置等待（如流式静默窗），不承担静息判定。
- **CAUSES 预填簇**：G-28（插件 TSX 有界 shim）、G-332（markdown 代码块
  有界行形状）、G-239/G-331（panel 有界偏离：宽度 docked/fullscreen、
  toggleFullscreen v1 省略）、G-368（量化色 ±8）。未覆盖分歧进
  "未裁决候选"，首轮人工归因后立新 G-ID。
- **边界声明**（范围外章节必写）：与 `/opt/opentui-rust` 127 场景差分的
  分工 = 框架层（opentui 库渲染语义）vs 应用层（opencode TUI 整屏）；
  动画中间帧（G-128/G-49/G-368 豁免）；活体交互序列（v2 扩展）。

## 9. 风险与对策

| 风险 | 等级 | 对策 |
|---|---|---|
| Zig 原生包（@opentui/core-linux-*） | 中 | tui-dump 已在本机证明可行；driver 探测失败=infra exit 1 |
| fetch 桩端点不全（TUI 实际请求集 /api/session、/api/project、/api/event、/config、/agent…） | 中 | 首轮用 driver 记录 ref 实际请求全集，逐端点补桩；漏桩=503 错误面板（本身也是一个场景） |
| 静息≠静态（spinner/相对时间） | 中 | animations:false + 固定时间注入 + rust 时钟 0；若仍有呼吸性字段，归一化剥换并登记 |
| opentui 半公开 API（currentRenderBuffer 等） | 低 | ref 树 pinned 2.0.24，不升级即不变 |
| rust 腿场景表达力（某些状态难以经 server 响应构造） | 低 | 允许场景带 `store_ops` 旁路字段直写 store（两侧语义等价性在场景注释里论证）；FakeLlm 全栈路径优先 |

## 10. 留给 builder 的

1. 场景脚本的 fetch 桩端点全集（以 ref TUI 实际请求探测为准）。
2. rust 侧脚本 → FakeLlm 桩/store 操作的映射实现细节（全栈优先、
   `store_ops` 旁路为辅，见风险 5）。
3. STRUCTURAL ±8 阈值的实测校准（先跑真彩 EXACT，看量化差分布再定）。
4. CAUSES 表 matcher 的具体写法（照通道 7 :348-393 lambda 模式）。

## 11. 验收（发给用户的完成标准）

1. `python3 tools/diff/tui_fullscreen.py` 幂等可复跑，产出六段报告。
2. 报告含：双侧驱动面 file:line、总量/EXACT/STRUCTURAL/未裁决四计数、
   分歧明细（含 first_diff 定位）、CAUSES 定性。
3. INDEX.md / GAPS 登记完成，范围外声明收窄。
4. **绿灯即交付（用户裁决 B，2026-11-13）**：v1 包含完整首轮裁决
   循环——每个分歧逐个归因（可修的修复并过双侧验证，有据分歧登记
   新 G-ID / 并入既有簇），直到未裁决 = 0、`exit 0` 门绿才算完。
   裁决期间发现的行为 bug 按 30 轮既有纪律处理（修复→nextest→
   重跑通道确认）；登记册同步记录本轮。
