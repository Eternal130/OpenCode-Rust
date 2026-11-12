# RESPONSES-COMPACTION Differential（差分层 · 通道 5）

> 同一 `response.compaction` corpus 喂双侧 解码→Message 管线：
> ref=bun 驱动（重建 responses-compaction.ts 私有 `Response` schema + `toMessage`，
> 从 open-responses.ts 导出的 schema 件 1:1 复原）；rust=生产
> `crates/oc-ai/src/protocols/utils/responses_compaction.rs::parse_compaction_response`
> （env 门控集成测试）。归一化：Message wire 形状（role/parts/providerMetadata）、
> 可选字段 null/undefined 键统一剥除、usage 原文透传剥除、错误归一为粗桶
> 复现：`python3 tools/diff/responses_compaction.py`（corpus：/opt/opencode-rust/tools/diff/corpus/compaction）

- **MATCH：42/48** · DIVERGE：**6** · MISSING：**0**（覆盖率 48/48）

| 用例 | 分歧字段 |
|---|---|
| `b21-no-checkpoint-and-bad-item.json` | reject |
| `b44-message-id-number.json` | accept, reject, payload |
| `b45-message-status-number.json` | accept, reject, payload |
| `v10-reasoning-id-null.json` | accept, reject, payload |
| `v14-message-user-image-dataurl-empty.json` | payload |
| `v19-usage-null.json` | accept, reject, payload |

## 分歧明细
### `b21-no-checkpoint-and-bad-item.json` — reject
- 定性：错误次序分歧（同为失败，错误类不同）：ref 先整体 schema decode 再查 checkpoint（responses-compaction.ts:140-144，缺 checkpoint+坏 item → schema 失败）；rust 先扫 checkpoint 再逐 item 映射（responses_compaction.rs:231-242，scan@234 在 map@239 之前 → no-checkpoint）。双侧均拒绝，桶不同。
- ref:
  - `reject` = `"item-schema"`
- rust:
  - `reject` = `"no-checkpoint"`

### `b44-message-id-number.json` — accept, reject, payload
- 定性：G-357 修复面残差：message.id 错型（数字）。ref 拒绝（responses-compaction.ts:54-59 MessageFields.id = Schema.optional(String)）；rust 接受并原样透传进 provider 元数据 itemId（responses_compaction.rs:365-373，仅 clone 不校验类型）。rust 候选：补 id/status 字符串校验。
- ref:
  - `accept` = `false`
  - `reject` = `"item-schema"`
  - `payload` = `null`
- rust:
  - `accept` = `true`
  - `reject` = `null`
  - `payload` = `{"messages": [{"parts": [{"encrypted": "enc-secret-1", "id": "cmp_1", "provider": "test-provider", "type": "compaction"}], "role": "assistant"}, {"parts": [{"text": "u", "type": "text"}], "providerMetadata": {"openresponses": {"itemId": 5, "type": "message"}}, "role": "user"}], "usage": {"inputTokens": 10.0, "nonCachedInputTokens": 10.0, "outputTokens": 2.0, "totalTokens": 12.0}}`

### `b45-message-status-number.json` — accept, reject, payload
- 定性：同 b44：message.status 错型（数字）。ref 拒绝（responses-compaction.ts:57 status = Schema.optional(String)）；rust 接受（responses_compaction.rs:370 原样 clone）。
- ref:
  - `accept` = `false`
  - `reject` = `"item-schema"`
  - `payload` = `null`
- rust:
  - `accept` = `true`
  - `reject` = `null`
  - `payload` = `{"messages": [{"parts": [{"encrypted": "enc-secret-1", "id": "cmp_1", "provider": "test-provider", "type": "compaction"}], "role": "assistant"}, {"parts": [{"text": "u", "type": "text"}], "providerMetadata": {"openresponses": {"itemId": "m", "status": 7, "type": "message"}}, "role": "user"}], "usage": {"inputTokens": 10.0, "nonCachedInputTokens": 10.0, "outputTokens": 2.0, "totalTokens": 12.0}}`

### `v10-reasoning-id-null.json` — accept, reject, payload
- 定性：reasoning.id 显式 null。ref 拒绝（open-responses.ts:85 id = Schema.optionalKey(String)，optionalKey 仅允许缺省、null 不是 String）；rust 接受（responses_compaction.rs:299 as_str→None，缺省/null 同路）。
- ref:
  - `accept` = `false`
  - `reject` = `"item-schema"`
  - `payload` = `null`
- rust:
  - `accept` = `true`
  - `reject` = `null`
  - `payload` = `{"messages": [{"parts": [{"encrypted": "enc-secret-1", "id": "cmp_1", "provider": "test-provider", "type": "compaction"}], "role": "assistant"}, {"parts": [{"providerMetadata": {"openresponses": {}}, "text": "T", "type": "reasoning"}], "role": "assistant"}], "usage": {"inputTokens": 10.0, "nonCachedInputTokens": 10.0, "outputTokens": 2.0, "totalTokens": 12.0}}`

### `v14-message-user-image-dataurl-empty.json` — payload
- 定性：空 data data-URL（data:image/png;base64,）。ref media.ts:290 DATA_URL 正则 `(.*)$` 允许空 payload → base64("", image/png)；rust schema/media.rs:342-352 parse_data_url 要求 data 非空 → None → 回退整串当 base64（data URL 原文成为 data，fallback 类型 image/*）。rust 候选：放开空 data 分支。
- ref:
  - `payload` = `{"messages": [{"parts": [{"encrypted": "enc-secret-1", "id": "cmp_1", "provider": "test-provider", "type": "compaction"}], "role": "assistant"}, {"parts": [{"media": {"data": "", "mediaType": "image/png", "source": "base64"}, "type": "media"}], "providerMetadata": {"openresponses": {"itemId": "msg_4", "type": "message"}}, "role": "user"}], "usage": {"inputTokens": 10, "nonCachedInputTokens": 10, "outputTokens": 2, "totalTokens": 12}}`
- rust:
  - `payload` = `{"messages": [{"parts": [{"encrypted": "enc-secret-1", "id": "cmp_1", "provider": "test-provider", "type": "compaction"}], "role": "assistant"}, {"parts": [{"media": {"data": "data:image/png;base64,", "mediaType": "image/*", "source": "base64"}, "type": "media"}], "providerMetadata": {"openresponses": {"itemId": "msg_4", "type": "message"}}, "role": "user"}], "usage": {"inputTokens": 10.0, "nonCachedInputTokens": 10.0, "outputTokens": 2.0, "totalTokens": 12.0}}`

### `v19-usage-null.json` — accept, reject, payload
- 定性：usage 显式 null。ref 拒绝（responses-compaction.ts:80 usage = Schema.optional(StructWithRest)，optional 仅允许缺省）；rust 接受并视为无 usage（responses_compaction.rs:245 filter(!is_null)→None）。
- ref:
  - `accept` = `false`
  - `reject` = `"item-schema"`
  - `payload` = `null`
- rust:
  - `accept` = `true`
  - `reject` = `null`
  - `payload` = `{"messages": [{"parts": [{"encrypted": "enc-secret-1", "id": "cmp_1", "provider": "test-provider", "type": "compaction"}], "role": "assistant"}]}`

## 判定

- 已裁定：48/48（每例双侧一致或分歧字段明确归属）
- 分歧 6 项为登记册候选/待裁定面（见上表明细，逐项人工定性后入册或背书）
- 已知锚：G-357 修复面（encrypted_content 必填 / 未知 item 拒绝 / role 字面量 / 非空 content）全部双侧对齐：b22-b42 中相关用例均 MATCH
- 已知锚：G-287⑦ minted-id：/compact 重放路径不铸 id（compaction item id 为 optionalNull，双侧一致透传），本通道未触及
