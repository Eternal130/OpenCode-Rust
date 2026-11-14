# 差分扩展三线（帧序列/插件量化/真实语料录制）Implementation Plan

> **For agentic workers:** 按任务逐个派 implementer + 评审（沿用通道 8 的 SDD 精简版：每任务独立验证 + 评审门）。

**Goal:** 补齐差分体系三个明知缺口：虚拟时钟帧序列对拍（动态语义）、插件 panel 差分（G-28 量化）、真实会话录制语料（本地隔离）。

**Architecture:** 全部建立在通道 8 既有设施上（场景格式/双侧 driver/编排器/CAUSES/登记纪律）。A 线新增帧序列维度；B 线复用 rust plugin_ui_slot harness + ref fixture 插件 boot；C 线加录制器→场景编译器，录制数据**只存本地 git-ignored 目录，永不入库/上传**。

**Spec:** 用户裁决 2026-11-14：三线全做；真实语料本地跑、数据不上传。通道 8 spec：docs/superpowers/specs/2026-11-13-tui-fullscreen-diff-channel-8-design.md（延续其反假绿/登记/幂等纪律）。

## Global Constraints

- 通道编排器不包 dev-gate；PIPESTATUS 记码；tee /tmp/opencode/logs/
- cargo 经 tools/dev-gate.sh；clippy -D warnings；fmt clean；guards 绿；英语 commit；Eternal130 身份
- ref 树只读；golden 不许自行重钉（需用户批准）
- **C 线隐私铁律**：录制数据目录 git-ignored（如 tools/diff/corpus/tui/recorded-local/ + .gitignore 条目），任何 commit 不得含录制内容；报告只允许聚合计数，不许引用录制内容原文
- 新场景键名/端点遵守 SCHEMA.md；CAUSES matcher 禁放宽

## 任务与依赖

### B1: 插件 boot 可行性探测（explorer，只读）
产出：ref fixture 能否加载真插件（feature-plugins/storybook 作载荷）的结论 + 双侧 harness 挂点 file:line + 载荷插件选型。无代码。

### A1: rust 时钟接线修复
vt_temporal 记录的 spinner/时钟未接线缺口——所有渲染路径的动画相位从 store 可注入时钟派生（ref ManualClock 对应物）。验收：vt_temporal known-gap 注释移除 + 既有测试绿 + 新增时钟步进单测。

### A2: 帧序列对拍模式
driver 增加 `frames` 输出（虚拟时钟 k=0..K 每拍整屏网格，K 场景声明默认 8）；编排器序列比对（逐拍 EXACT + 首异拍定位）；CAUSES 预填 G-128/G-49/G-368 族（节奏类=登记不拦，形状类=拦）；动画场景族入语料（spinner/流式 delta/加载条）。首轮裁决循环至门绿或有据登记。

### B2: 真插件差分场景（用户裁决 2026-11-14）
载荷=用户真实插件集：omo-slim 3.0.3（npm 实体在 /root/.cache/opencode/npm/oh-my-opencode-slim@latest/...，server+TUI 双入口）+ superpowers 7.0.0（git 实体，纯 skill 系统）。差分钉 ref 2.0.24；插件 boot 成败以 fixture 实测为准（静态分析对 minified bundle 不可靠）。**分歧甄别新增"退版本加载伪差"族**：插件落 2.0.24 旧 API 的降级伪影（如 ctx.client/directory/hostFlavor 缺失路径、TUI 2.2.x API 降级）须与移植 bug 区分——CAUSES 加 version-skew 簇（判定文引插件调用点+2.0.24 API 面），命中=登记不拦但须写明伪差机理。场景：插件加载静息屏（palette 含插件命令/keymap 层/omo-slim 侧栏 slot）+ panel 打开屏。G-28 量化清单随裁决产出；INDEX/GAPS 更新。

### A3: 双 PTY rig 收编（可选收官）
tools/parity/src/tui_diff.rs + p10_cross_tui 并入通道体系为"活体模式"（opt-in 子命令，报告独立小节）；登记收编记录。

### C1: 会话录制器（env 门控）
oc-tui env `TUI_RECORD_DIR` 开启：真实会话的 SSE 事件流+键序+fetch 应答摘要落为场景 JSON（通道 8 格式）；只写本地目录；含尺寸档记录。红线：文件名时间戳+会话 id，不含其他元数据。

### C2: 场景编译器 + 本地语料目录
录制原始件 → 可重放场景（端点普查对齐、事件归一复用 w3_normalize 逻辑）；`tools/diff/corpus/tui/recorded-local/`（git-ignored）；编排器自动纳入该目录（存在即跑，报告单列"recorded-local"小节聚合计数）；README 写隐私铁律。

### C3: 真实语料首轮回放验证
用录制器采 3-5 个真实会话（含你当前使用模式）→ 编译 → 本地全量重放 → 分歧裁决（预期会抓真问题：真实事件序列的组合远超手造语料）→ 聚合结果登记（GAPS 只记计数与结论，不记内容）。

## 执行序
B1（探测，与 A1 无依赖但串行纪律）→ A1 → A2 → B2 → C1 → C2 → C3 →（A3 可选）→ 终审 → 并入 main。

## 验收
各线门绿或有据登记；三连幂等；套件/fmt/clippy/guards 绿；录制数据零入库（git 验证）；INDEX/GAPS 终态。
