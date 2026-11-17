# Differential INDEX —— 差分层总索引（9 通道 · 语料升级版）

> 语义等价性差分：同一 corpus 喂双侧真实实现（纯函数管线或真实进程），归一化后逐字段对拍。
> 与 census 的分工：census 证"面存在且键集对齐"，差分证"行为等价"。
> 全部脚本幂等、exit 0、双侧 file:line 证据。本轮（2026-11-13）完成 **ref 测试夹具挖掘**：123 个 mined 语料 + 13 个 HTTP 请求入既有通道，274 个 provider 录制落成**新通道 7**——语料来源从"自造"升级为"ref 验证过的输入集 + 真实 provider 流量"。

## 通道总表

| # | 通道 | 脚本 | 报告 | 规模 | MATCH | DIVERGE | ADJUDICATED |
|---|---|---|---|---|---|---|---|
| 1 | 配置解析 | config_parse.py | CONFIG-PARSE.md | 78 例（35 mined） | 77 | 1 | — |
| 2 | ACP 转译 | acp_translate.py | ACP-TRANSLATE.md | 137 例（40 mined） | 111 | 26 | — |
| 3 | SSE store 投影 | sse_projection.py | SSE-PROJECTION.md | 15 序列/107 事件 | 2 | 13 | — |
| 4 | JSON Schema 校验器 | json_schema_validate.py | JSON-SCHEMA.md | 175 对（23 mined） | 156 | 19 | — |
| 5 | /compact 管线 | responses_compaction.py | RESPONSES-COMPACTION.md | 61 文档（13 mined） | 60 | 1 | — |
| 6 | HTTP 全量 | http_full.py | HTTP-FULL.md | 60 请求（13 mined） | 47 | 11 | 2 |
| 7 | **provider 录制解码** | provider_recordings.py | PROVIDER-RECORDINGS.md | **264 交互（259 真实 SSE）** | 264 | 0 | — |
| 8 | TUI 整屏差分（静息帧 + A2 帧序列 + B2 真插件） | tui_fullscreen.py | TUI-FULLSCREEN.md | 静息 90 case（30 场景×3 档，含 plugin-* 15）+ frames 15 序列（5 anim×3 档）；插件腿 TUI_DIFF_BUN≥1.4.2 门控 | 静息 **EXACT 90**；frames **EXACT 15** | 0（G-28 有界 shim 簇 gb17 W5 修复关簇——桥接 walk 盒垂向度量空行模型 + 双侧 XDG 密闭化；W4 已关 G-424b、退役 B2 投影族） | 0 committed + rec-* advisory 9/9 exact |
| 9 | **真实链路对拍（W3 起）** | replay_gateway.py + `crates/oc-tui/tests/realchain_{t9,pair,toolloop}.rs` | .superpowers/sdd/r1/w{1..7}-report.md + GAPS 第三十五/三十六轮 | T9 矩阵 7 测（W6 +t9_8 慢启动窗口；真 PTY→真二进制→真托管 oc-server→回放网关唯一桩）+ pair 六场景 S1-S6（双端：ref 2.0.24 bun 与 rust 同网关同键序，归一化 grid 对拍）† + **W7 工具环 `tl_tool_loop_pair`（网关 v4 回合锚定 + PATH shim 快执行 + 权限预授权：hello→shell cargo test→edit→确认，双端全链路）** | **S1-S6 + tl 全 EXACT（零掩码——G-433/435/436/437/441/442 掩码与绊线已随 gwave-1 修复波全部退役，被掩形状转为双侧一致闭合钉）** | 在役仅 G-434（models.dev 快照 vintage——评估：数据刷新依赖）与 G-442 🟡（shell 行快完成投影竞态——W9 根因修后残留竞态面已收窄） | G-431/G-431b/G-432/G-435/G-436/G-439/G-440/G-441/G-433/G-437 已修关 |

<!-- † 通道 9 脚注（W7，2026-11-17）：T9 矩阵中 t9_8（慢启动 boot 窗口）是**单端（rust-only）**测试——其 ref 侧锚定于源码级证据（ref 托管链路的启动窗口本就无 "Connection lost" 文案：CLI 先健康门禁再挂载 server-connection.ts:46-50），只读 ref 树无法注入同款 SSE 首帧延迟（OPENCODE_TEST_SSE_FIRST_FRAME_DELAY_MS 是 rust oc-server 的测试钩子）；pair 双端对拍由 S1-S6/P1-P4b 承担。 -->


## 总计

**通道 1-7：790 对拍单元：MATCH 717 · DIVERGE 71 · ADJUDICATED 2 · MISSING 0**（100% 三态归属；第三十轮 burndown 收口口径）。**通道 8（T9 裁决循环 2026-11-14 + T8-320 档 + A2 帧序列 + B2 真插件 2026-11-15 修复轮 + gap-burndown-16 + gb17 2026-11-16 四波清偿）：静息 90 case（30 场景×3 档）：EXACT 90 · STRUCTURAL 0 · CAUSES 0 · 未裁决 0——**通道史上首次全一致**（G-423/G-424/G-425/G-426/G-427/G-424b 先后修复关簇；B2 加载模型投影族（gb17 W4）按裁决文镜像退役；G-28 有界 shim 残余单例（gb17 W5）修复关簇——桥接 walk 盒垂向度量→空行模型 + 双腿 XDG_CONFIG_HOME 密闭化）；frames 15 序列：EXACT 15 · CAUSES 0——门绿 exit 0，幂等（TUI_DIFF_BUN 设定下）。recorded-local（rec-* advisory 不拦门）：rec- 9/9 全 EXACT**——合计 880 对拍单元（+ frames 15 + rec-* advisory 9）。**通道 9（2026-11-17 立项 W1-W5）：真实链路差分——同网关双端（ref 2.0.24 bun / rust 生产二进制）同键序，settled grid 归一化对拍：T9 矩阵 6 测沙箱绿 + pair 场景 EXACT（S1-S6 + W5 时序 P1-P3 + 插件 T9-7 + 侧栏深比对 P4/P4b；gwave-1 修复波后 S5/S6/tl 零掩码全网格比对——G-433/435/436/437/441/442 掩码全部退役转闭合钉；P4b 120 列档零掩码）；W5：网关 pace 引擎（instant/frames/synthetic——TTFT/TPS 确定性档型，双端同 schedule）+ G-438 修复（`session.step.failed` error 投影缺失——` · interrupted` 徽章）+ 真插件 rig（omo-slim 3.0.3+superpowers 7.0.0 真实 config 发现路径，症状⑦ port 侧未复现）+ G-435 narrowing（Context 块滞后一回合）；**W6：三真机症状修复——G-439（never-connected boot "Connection lost…" 覆盖层：ever-connected 闩，t9_8 慢 SSE 注入场景修复前 RED 实证）、G-440（鼠标圈选反显高亮：`selection.rs::paint_highlight` 后网格通道）、G-128 勘注升级（busy 呼吸灯已在树，帧序列 `busy_pulse_frames.rs` 钉死）**；CI 语义：T9 进常规 nextest job（bin 由 build job 构建+strip 进共享 artifact），pair 依赖 ref 树/bun=缺席 skip（ref_lane_skip_reason），T9-7 依赖插件供给 staging=缺席 skip（W8 起供给持久化于仓内 git-ignored `tools/diff/realchain/plugin-payload/`）；**W8 CI 化收官：手动 workflow `realchain-diff.yml`（workflow_dispatch）=全 lane 真链路对拍上 CI——ref 2.0.24 tag tarball 装临时 `T9_REF_ROOT`+pinned bun 1.4.2（pair/toolloop lane 就位不 skip）；默认派发=纯合成语料零 secret；`corpus_bundle`+`CORPUS_AGE_KEY`=age 解密注入真录制语料（缺 secret 且指定 bundle=红），结果 dumps 以 `CORPUS_AGE_PUBKEY` age 加密回传；加密语料经 `corpus_age.sh` 管线入 `corpus/encrypted/`（明文禁入=recorded_local 守卫）**。
<!-- 历史脚注（第二十九轮基线口径，已被上方第三十轮收口数字取代，保留备查）：
     101 项分歧：既有 78（R27/R28 裁定族）+ 挖掘新增 23 → 归并 G-412..G-422（11 条：🟡×1 / 🟢×10），其余归 G-407/409/410 既有族。 -->

## 挖掘新增发现（关键项）

- **G-412 🟡 abandon 输出顺序不定**：rust 迭代 HashMap → tool 更新 wire 顺序**跨 run 不稳定**且与 ref 插入序反
- G-413 v1 permission 迁移条目重排（serde 无 preserve_order 的可观察面）
- G-419/G-420 providerMetadata 空 namespace 附着方向相反（venice rust 恒附 / google ref 恒附，13+6 交互）
- G-417 compaction `detail` 非字符串被静默吞掉（ref 拒绝）
- G-422 deepinfra estimated_cost 浮点精度（疑似 f32 截断）

## 已知锚核对（挖掘轮）

G-357/G-392/G-393/G-394/G-396/G-397 修复面在 mined 语料下保持对齐 ✅ · ref 自身畸形 config 样本（`"{ invalid"`/`$:invalid`）双侧对齐 ✅ · tool-input-repair 的 (schema,input) 对全部对齐 ✅

## 语料来源构成

通道 1-6：手造边界 + schema 推导 + 登记册锚 + **mined（ref 测试夹具：config 测试写盘样本/translate 表/tool-input-repair 对/open-responses 内联事件/server 路由测试）**；通道 7：**HttpRecorder cassette 全量**（ref 真实 provider 流量录制，220 SSE + 错误体；bedrock event-stream/websocket/vercel-native/cf-gateway/媒体族=范围外 32 组已声明）。
挖掘器：`tools/diff/mine_ref_fixtures.py`（幂等；注意 gen_acp_corpus.py 再生成会清空 mined，重跑挖掘器即恢复）；通道 8：自造场景脚本 20×3（80x24/120x30/320x80 三档，T8-320 扩容=用户裁决的 4K 全屏压力档、自动覆盖 sidebar auto-dock regime），双侧真实渲染（rust nextest 驱动 × ref bun staging 驱动）。

## 范围外声明（各报告详述）

通道 8 覆盖渲染层**无头静息整屏**（25 场景×3 档，320×80 档自动覆盖 sidebar auto-dock 布局 regime：session-frame.tsx wide()）+ **A2 帧序列对拍**（anim 场景 rust 虚拟时钟逐拍 advance_clock(k·100ms)，frames 维度逐拍 EXACT + 首异拍定位；动画中间帧自 A2 起部分可断言——G-128 midframe 豁免收窄为「首末拍一致、仅中间拍差」精确形状）；渲染层范围外余量：ref 侧虚拟时钟不可注入面（ManualClock 整 app 注入原生不稳：toast 真实 setTimeout / caret 原生光标 / animations:true 捕获段错误——夹具伪影族登记）、活体交互序列（v2 扩展，parity::tui_diff 双 PTY rig 存在未收编）、mini 模式 footer（ref 独立入口 fixture 不可装配）、插件 TSX panel/plugin-*（两侧驱动无插件 boot，G-28 登记面由 G-332 markdown 有界场景部分承担）· ref message.updated 全量投影 · session.moved 纯事件流门控 · HTTP 鉴权态/workerd/网络出口 · bedrock 二进制 event-stream/websocket/gateway 包裹族（通道 7）· minted-id（G-287⑦）
