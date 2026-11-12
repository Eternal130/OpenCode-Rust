# Differential INDEX —— 差分层总索引（6 通道）

> 语义等价性差分：同一 corpus 喂双侧真实实现（纯函数管线或真实进程），归一化后逐字段对拍。
> 与 census 的分工：census 证"面存在且键集对齐"，差分证"行为等价"。
> 全部脚本幂等（连跑逐字节一致）、exit 0、双侧 file:line 证据。生成于 2026-11-12（round-23/25/26 修复后基线）。

## 通道总表

| # | 通道 | 脚本 | 报告 | 规模 | MATCH | DIVERGE | ADJUDICATED |
|---|---|---|---|---|---|---|---|
| 1 | 配置解析 | config_parse.py | CONFIG-PARSE.md | 43 例 | 42 | 1（G-390 已裁决旋钮） | — |
| 2 | ACP 转译 | acp_translate.py | ACP-TRANSLATE.md | 97 例（51/51 事件类型） | 78 | 19（3 缺口+3 有据+噪声已消解） | — |
| 3 | SSE store 投影 | sse_projection.py | SSE-PROJECTION.md | 12 序列/93 事件 | 1 | 11（8 成因族） | — |
| 4 | JSON Schema 校验器 | json_schema_validate.py | JSON-SCHEMA.md | 152 对/54 schema | 128 | 24（双向各 12） | — |
| 5 | /compact 管线 | responses_compaction.py | RESPONSES-COMPACTION.md | 48 文档/86 item | 42 | 6 | — |
| 6 | HTTP 全量 | http_full.py | HTTP-FULL.md | 47 请求/4 族（双侧真实 boot） | 32 | 13 | 2 |

## 总计

**384 个对拍单元：MATCH 323 · DIVERGE 74 · ADJUDICATED 2 · MISSING 0**（覆盖率 100%：每单元 MATCH/DIVERGE/ADJUDICATED 三态归属）。
74 项分歧归并为 **20 个登记册条目（G-392..G-411：🟡×6 / 🟢×9 / ℹ️×5）**——见登记册第二十七轮。

## 关键发现（按严重度）

1. **ACP child toolCallId 双重前缀**（G-392，wire bug）：`pending_update` 前缀一次 + `route→child::project` 再前缀一次
2. **SSE reasoning 空白 delta 丢弃**（G-393，G-351 姊妹缺口：只修了 text 臂）
3. **HTTP provider.list wire 形状整差**（G-394：rust 全量目录无包裹 vs ref 仅启用+location 包裹）
4. **JSON Schema 校验器 7 关键字缺口**（G-395：prefixItems 及其 items 副作用、exclusiveMin/Max、multipleOf、minProperties、schema:false、畸形 type）
5. **ref 生产校验器把编译错误洗成 accept-all**（G-407：pattern/additionalProperties:false/结构化 const 在 ref 生产根本不校验——rust 更严属规范正确，登记为有意分歧）

## 已知锚核对

G-357（compact 严格化）四修复位全对齐 ✅ · G-351 半边对齐（text ✅ / reasoning ❌→G-393）· G-352 六类 live 事件对齐（终态口径差→G-409）· G-339/G-340（ACP 预览/ISO 串）对齐 ✅ · G-230c（const）对齐 ✅ · G-288⑧（乱序补位）产物显现并定向 ✅

## 范围外声明（各报告详述）

渲染层（opentui 差分 127 场景另行覆盖）· ref message.updated 全量投影（app 层）· session.moved 纯事件流门控（需 API 装载）· HTTP 鉴权态/workerd 剖面/网络出口 · minted-id（G-287⑦ 已记录）· ref 校验器内部（effect node_modules，以 runtime.ts 消费方式为锚）
