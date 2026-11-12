# CONFIG-PARSE Differential（差分层 · 通道 1）

> 同一 corpus 喂双侧纯函数管线（jsonc-parse → normalize → Info decode），
> 归一化（kind 小写、诊断排序、encoded 键序排序）后逐字段对拍。
> 驱动：ref=bun（staging+node_modules 链接）/ rust=env 门控集成测试。
> 复现：`python3 tools/diff/config_parse.py`（corpus：/opt/opencode-rust/tools/diff/corpus/config）

- **MATCH：41/43** · DIVERGE：**2** · MISSING：**0**（覆盖率 43/43）

| 用例 | 分歧字段 |
|---|---|
| `13-provider-models.json` | encoded |
| `16-experimental.json` | encoded |

## 分歧明细
### `13-provider-models.json` — encoded
- ref:
  - `encoded` = `{"providers": {"myprov": {"models": {"m1": {"name": "M1"}}}}}`
- rust:
  - `encoded` = `{"providers": {"myprov": {"models": {"m1": {"cost": null, "name": "M1"}}}}}`

### `16-experimental.json` — encoded
- ref:
  - `encoded` = `{}`
- rust:
  - `encoded` = `{"experimental": {"file_retention": {"cleanup_interval": 60}}}`

## 判定

- 已裁定：43/43（每例双侧一致或分歧字段明确归属）
- 分歧 2 项为登记册候选/待裁定面（见上表明细，逐项人工定性后入册或背书）
