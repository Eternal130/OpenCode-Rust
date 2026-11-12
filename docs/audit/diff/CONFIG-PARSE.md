# CONFIG-PARSE Differential（差分层 · 通道 1）

> 同一 corpus 喂双侧纯函数管线（jsonc-parse → normalize → Info decode），
> 归一化（kind 小写、诊断排序、encoded 键序排序）后逐字段对拍。
> 驱动：ref=bun（staging+node_modules 链接）/ rust=env 门控集成测试。
> 复现：`python3 tools/diff/config_parse.py`（corpus：/opt/opencode-rust/tools/diff/corpus/config）

- **MATCH：42/43** · DIVERGE：**1** · MISSING：**0**（覆盖率 43/43）

| 用例 | 分歧字段 |
|---|---|
| `16-experimental.json` | encoded |

## 分歧明细
### `16-experimental.json` — encoded
- ref:
  - `encoded` = `{}`
- rust:
  - `encoded` = `{"experimental": {"file_retention": {"cleanup_interval": 60}}}`

## 判定

- 已裁定：43/43（每例双侧一致或分歧字段明确归属）
- 分歧 1 项为登记册候选/待裁定面（见上表明细，逐项人工定性后入册或背书）
- 手工裁定注（回贴节，脚本再生成后需重贴——体例同 HTTP-SURFACE）：
  - `13-provider-models` 原 `"cost": null` 物化分歧已由 **G-391** 修复闭合并于 2026-11-12 复跑转 MATCH（commit `568213c8`）。
  - `16-experimental` 残余分歧＝**G-390** 已裁定项：rust 超集旋钮 `experimental.file_retention.cleanup_interval` 的预期表现（ref 静默剥除未知 experimental 键、rust 保留并消费），有据分歧非缺口，登记册背书。
