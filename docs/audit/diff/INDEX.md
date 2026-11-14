# Differential INDEX —— 差分层总索引（8 通道 · 语料升级版）

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
| 8 | TUI 整屏差分（无头静息帧） | tui_fullscreen.py | TUI-FULLSCREEN.md | 40 case（20 场景×2 档） | **EXACT 40** | 未裁决 0 | — |

## 总计

**通道 1-7：790 对拍单元：MATCH 717 · DIVERGE 71 · ADJUDICATED 2 · MISSING 0**（100% 三态归属；第三十轮 burndown 收口口径）。**通道 8（T9 裁决循环四波收口，2026-11-14）：40 case：EXACT 40 · STRUCTURAL 0 · CAUSES 0 · 未裁决 0——门绿 exit 0，无新 G-ID**（详见 TUI-FULLSCREEN.md 与 GAPS-AND-PITFALLS 第三十一轮）——合计 830 对拍单元。
101 项分歧：既有 78（R27/R28 裁定族）+ 挖掘新增 23 → 归并 **G-412..G-422（11 条：🟡×1 / 🟢×10）**，其余归 G-407/409/410 既有族。

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
挖掘器：`tools/diff/mine_ref_fixtures.py`（幂等；注意 gen_acp_corpus.py 再生成会清空 mined，重跑挖掘器即恢复）；通道 8：自造场景脚本 20×2（80x24/120x30 两档），双侧真实渲染（rust nextest 驱动 × ref bun staging 驱动）。

## 范围外声明（各报告详述）

通道 8 覆盖渲染层**无头静息整屏**（20 场景×2 档）；渲染层范围外余量：动画中间帧（G-128/G-49/G-368 文档化豁免）、活体交互序列（v2 扩展，parity::tui_diff 双 PTY rig 存在未收编）、mini 模式 footer（ref 独立入口 fixture 不可装配）、插件 TSX panel/plugin-*（两侧驱动无插件 boot，G-28 登记面由 G-332 markdown 有界场景部分承担）· ref message.updated 全量投影 · session.moved 纯事件流门控 · HTTP 鉴权态/workerd/网络出口 · bedrock 二进制 event-stream/websocket/gateway 包裹族（通道 7）· minted-id（G-287⑦）
