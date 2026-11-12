# RESPONSES-COMPACTION Differential（差分层 · 通道 5）

> 同一 `response.compaction` corpus 喂双侧 解码→Message 管线：
> ref=bun 驱动（重建 responses-compaction.ts 私有 `Response` schema + `toMessage`，
> 从 open-responses.ts 导出的 schema 件 1:1 复原）；rust=生产
> `crates/oc-ai/src/protocols/utils/responses_compaction.rs::parse_compaction_response`
> （env 门控集成测试）。归一化：Message wire 形状（role/parts/providerMetadata）、
> 可选字段 null/undefined 键统一剥除、usage 原文透传剥除、错误归一为粗桶
> 复现：`python3 tools/diff/responses_compaction.py`（corpus：/opt/opencode-rust/tools/diff/corpus/compaction）

- **MATCH：47/48** · DIVERGE：**1** · MISSING：**0**（覆盖率 48/48）

| 用例 | 分歧字段 |
|---|---|
| `b21-no-checkpoint-and-bad-item.json` | reject |

## 分歧明细
### `b21-no-checkpoint-and-bad-item.json` — reject
- 定性：错误次序分歧（同为失败，错误类不同）：ref 先整体 schema decode 再查 checkpoint（responses-compaction.ts:140-144，缺 checkpoint+坏 item → schema 失败）；rust 先扫 checkpoint 再逐 item 映射（responses_compaction.rs:231-242，scan@234 在 map@239 之前 → no-checkpoint）。双侧均拒绝，桶不同。
- ref:
  - `reject` = `"item-schema"`
- rust:
  - `reject` = `"no-checkpoint"`

## 判定

- 已裁定：48/48（每例双侧一致或分歧字段明确归属）
- 分歧 1 项为登记册候选/待裁定面（见上表明细，逐项人工定性后入册或背书）
- 已知锚：G-357 修复面（encrypted_content 必填 / 未知 item 拒绝 / role 字面量 / 非空 content）全部双侧对齐：b22-b42 中相关用例均 MATCH
- 已知锚：G-287⑦ minted-id：/compact 重放路径不铸 id（compaction item id 为 optionalNull，双侧一致透传），本通道未触及
