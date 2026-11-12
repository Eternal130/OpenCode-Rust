# JSON-SCHEMA Differential（差分层 · 通道 4）

> 同一 (schema, instance) 对喂双侧 JSON Schema 校验器，diff 接受/拒绝判定。
> rust：`crates/oc-ai/src/json_schema.rs validate_input`（G-59/G-100/G-230 共享引擎，纯函数，
> `cargo test -p oc-ai --test json_schema_diff_driver` env 门控驱动）。
> ref：effect `JsonSchema` → `SchemaRepresentation.fromJsonSchemaDocument` 编译 codec，
> 镜像 `packages/core/src/tool/runtime.ts` validateInput 的原始 JsonSchema 路径
>（含 Cache 查找 catch→undefined→accept 的宽松语义）。bun 驱动逐对校验。
> 复现：`python3 tools/diff/json_schema_validate.py`（corpus：/opt/opencode-rust/tools/diff/corpus/json_schema）

- **MATCH：128/152** · rust-accept-ref-reject：**12** · rust-reject-ref-accept：**12** · MISSING：**0**

## 关键字覆盖矩阵

| 关键字 | 对数 | MATCH | rust宽松 | rust严格 |
|---|---|---|---|---|
| `$defs` | 7 | 7 | 0 | 0 |
| `$ref` | 7 | 7 | 0 | 0 |
| `additionalProperties` | 8 | 5 | 0 | 3 |
| `allOf` | 5 | 5 | 0 | 0 |
| `anyOf` | 6 | 6 | 0 | 0 |
| `const` | 11 | 9 | 0 | 2 |
| `enum` | 5 | 4 | 0 | 1 |
| `exclusiveMaximum` | 2 | 1 | 1 | 0 |
| `exclusiveMinimum` | 3 | 1 | 2 | 0 |
| `format` | 3 | 3 | 0 | 0 |
| `integer` | 13 | 9 | 3 | 1 |
| `items` | 8 | 7 | 0 | 1 |
| `malformed-schema` | 11 | 9 | 0 | 2 |
| `maxItems` | 2 | 2 | 0 | 0 |
| `maxLength` | 3 | 3 | 0 | 0 |
| `maximum` | 6 | 6 | 0 | 0 |
| `minItems` | 3 | 3 | 0 | 0 |
| `minLength` | 7 | 7 | 0 | 0 |
| `minProperties` | 2 | 1 | 1 | 0 |
| `minimum` | 5 | 5 | 0 | 0 |
| `multipleOf` | 3 | 1 | 2 | 0 |
| `number` | 4 | 4 | 0 | 0 |
| `oneOf` | 5 | 5 | 0 | 0 |
| `pattern` | 5 | 3 | 0 | 2 |
| `prefixItems` | 5 | 2 | 2 | 1 |
| `properties` | 12 | 12 | 0 | 0 |
| `recursion` | 4 | 4 | 0 | 0 |
| `required` | 6 | 6 | 0 | 0 |
| `type` | 42 | 36 | 3 | 3 |
| `union` | 10 | 10 | 0 | 0 |
| `uniqueItems` | 4 | 3 | 1 | 0 |

## 分歧清单

| 对 | 方向 | 定性 |
|---|---|---|
| `additional-properties-false/extra` | rust-reject-ref-accept | effect SchemaRepresentation 对 additionalProperties:false 不产生封闭对象校验（probe 直证 ACCE |
| `additional-properties-false/only-extra` | rust-reject-ref-accept | 同上（additionalProperties:false 未被 effect 实施）。 |
| `additional-properties-schema/bad-extra` | rust-reject-ref-accept | 同上（additionalProperties 子 schema 未被 effect 实施）。 |
| `const-object/extra` | rust-reject-ref-accept | 同上（结构化 const 编译失败）。 |
| `const-object/str-val` | rust-reject-ref-accept | effect 对结构化（对象/数组）const 编译抛错（"Unsupported structured JSON Schema value for const |
| `enum-mixed/float-1` | rust-reject-ref-accept | enum 含 u64(1)，实例 f64(1.0)，serde_json Number 按变体判等→不等；JS 数值 1===1.0。与上一条同根因。 |
| `exclusive-maximum/ten` | rust-accept-ref-reject | rust 未实现 exclusiveMaximum。 |
| `exclusive-minimum/neg` | rust-accept-ref-reject | rust 未实现 exclusiveMinimum。 |
| `exclusive-minimum/zero` | rust-accept-ref-reject | rust 未实现 exclusiveMinimum。 |
| `integer-big-int/big` | rust-accept-ref-reject | effect integer AST 以 JS 安全整数域为界，2^53 起拒绝（probe 直证）；rust u64/i64 覆盖全 64 位。另注：JS J |
| `integer-big-int/big-ok` | rust-accept-ref-reject | 2^53 恰可精确表示但 effect 仍拒绝——安全整数边界取 < 2^53。 |
| `integer-big-int/neg-big` | rust-accept-ref-reject | 同上，负方向对称。 |
| `min-properties/empty` | rust-accept-ref-reject | rust 未实现 minProperties。 |
| `multiple-of/float` | rust-accept-ref-reject | rust 未实现 multipleOf。 |
| `multiple-of/odd` | rust-accept-ref-reject | rust 未实现 multipleOf。 |
| `pattern/space` | rust-reject-ref-accept | 同上（pattern 编译抛错→catch→accept-all）。 |
| `pattern/upper` | rust-reject-ref-accept | effect Draft2020-12 编译器对 pattern 默认抛错（"Pattern encountered while patterns is set |
| `prefix-items-plus-items/ok` | rust-reject-ref-accept | items 被应用到全部元素，前缀位置的 "a" 也按 {type:"integer"} 校验→误拒合法输入。 |
| `prefix-items/bad-first` | rust-accept-ref-reject | rust 未实现 prefixItems（关键字被忽略），draft-2020-12 语义丢失。 |
| `prefix-items/bad-second` | rust-accept-ref-reject | 同上（prefixItems 未实现）。 |
| `type-integer/float-int` | rust-reject-ref-accept | serde_json 保留 `5.0` 为 f64（is_i64=false→非 integer），JS 中 5.0===5 为整数。数字表示差，非能力缺口。 |
| `type-malformed-name/num` | rust-reject-ref-accept | 同上。 |
| `type-malformed-name/str` | rust-reject-ref-accept | rust 将未知 type 名当作永不匹配→拒绝一切；ref 编译 codec 失败→Cache catch→accept-all。双侧都不是 spec 行为； |
| `unique-items/int-float-dup` | rust-accept-ref-reject | serde_json 1(u64) 与 1.0(f64) 判不相等→非重复；JS 1===1.0 判重复。与数字表示差同根因。 |

## 分歧明细
### `additional-properties-false/extra` — rust-reject-ref-accept
- 关键字：`additionalProperties`
- rust 锚点：`additionalProperties`→json_schema.rs:289-317
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `False` · ref accept = `True`
- 定性：记录性宽松差（ref 侧，重大）：effect SchemaRepresentation 对 additionalProperties:false 不产生封闭对象校验（probe 直证 ACCEPT）；rust 执行拒绝。

### `additional-properties-false/only-extra` — rust-reject-ref-accept
- 关键字：`additionalProperties`
- rust 锚点：`additionalProperties`→json_schema.rs:289-317
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `False` · ref accept = `True`
- 定性：记录性宽松差（ref 侧）：同上（additionalProperties:false 未被 effect 实施）。

### `additional-properties-schema/bad-extra` — rust-reject-ref-accept
- 关键字：`additionalProperties`
- rust 锚点：`additionalProperties`→json_schema.rs:289-317
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `False` · ref accept = `True`
- 定性：记录性宽松差（ref 侧）：同上（additionalProperties 子 schema 未被 effect 实施）。

### `const-object/extra` — rust-reject-ref-accept
- 关键字：`const`
- rust 锚点：`const`→json_schema.rs:244-254 (G-230)
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `False` · ref accept = `True`
- 定性：记录性宽松差（ref 侧）：同上（结构化 const 编译失败）。

### `const-object/str-val` — rust-reject-ref-accept
- 关键字：`const`
- rust 锚点：`const`→json_schema.rs:244-254 (G-230)
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `False` · ref accept = `True`
- 定性：记录性宽松差（ref 侧）：effect 对结构化（对象/数组）const 编译抛错（"Unsupported structured JSON Schema value for const"）→ catch → accept-all；rust 深比较拒绝。G-230 的 const 能力面 rust 更完备。

### `enum-mixed/float-1` — rust-reject-ref-accept
- 关键字：`enum`
- rust 锚点：`enum`→json_schema.rs:255-265
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `False` · ref accept = `True`
- 定性：记录性严格差：enum 含 u64(1)，实例 f64(1.0)，serde_json Number 按变体判等→不等；JS 数值 1===1.0。与上一条同根因。

### `exclusive-maximum/ten` — rust-accept-ref-reject
- 关键字：`exclusiveMaximum`
- rust 锚点：`exclusiveMaximum`→json_schema.rs — NOT IMPLEMENTED (absent keyword)
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `True` · ref accept = `False`
- 定性：缺口候选：rust 未实现 exclusiveMaximum。

### `exclusive-minimum/neg` — rust-accept-ref-reject
- 关键字：`exclusiveMinimum`
- rust 锚点：`exclusiveMinimum`→json_schema.rs — NOT IMPLEMENTED (absent keyword)
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `True` · ref accept = `False`
- 定性：缺口候选：rust 未实现 exclusiveMinimum。

### `exclusive-minimum/zero` — rust-accept-ref-reject
- 关键字：`exclusiveMinimum`
- rust 锚点：`exclusiveMinimum`→json_schema.rs — NOT IMPLEMENTED (absent keyword)
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `True` · ref accept = `False`
- 定性：缺口候选：rust 未实现 exclusiveMinimum。

### `integer-big-int/big` — rust-accept-ref-reject
- 关键字：`type`, `integer`
- rust 锚点：`type`→json_schema.rs:146-170 (union parse) / :31-43 (matches_type); `integer`→json_schema.rs:34-36
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `True` · ref accept = `False`
- 定性：记录性严格差（ref 侧）：effect integer AST 以 JS 安全整数域为界，2^53 起拒绝（probe 直证）；rust u64/i64 覆盖全 64 位。另注：JS JSON.parse 将 9007199254740993 读成 9007199254740992（提取噪声）。

### `integer-big-int/big-ok` — rust-accept-ref-reject
- 关键字：`type`, `integer`
- rust 锚点：`type`→json_schema.rs:146-170 (union parse) / :31-43 (matches_type); `integer`→json_schema.rs:34-36
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `True` · ref accept = `False`
- 定性：记录性严格差（ref 侧）：2^53 恰可精确表示但 effect 仍拒绝——安全整数边界取 < 2^53。

### `integer-big-int/neg-big` — rust-accept-ref-reject
- 关键字：`type`, `integer`
- rust 锚点：`type`→json_schema.rs:146-170 (union parse) / :31-43 (matches_type); `integer`→json_schema.rs:34-36
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `True` · ref accept = `False`
- 定性：记录性严格差（ref 侧）：同上，负方向对称。

### `min-properties/empty` — rust-accept-ref-reject
- 关键字：`minProperties`
- rust 锚点：`minProperties`→json_schema.rs — NOT IMPLEMENTED (absent keyword)
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `True` · ref accept = `False`
- 定性：缺口候选：rust 未实现 minProperties。

### `multiple-of/float` — rust-accept-ref-reject
- 关键字：`multipleOf`
- rust 锚点：`multipleOf`→json_schema.rs — NOT IMPLEMENTED (absent keyword)
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `True` · ref accept = `False`
- 定性：缺口候选：rust 未实现 multipleOf。

### `multiple-of/odd` — rust-accept-ref-reject
- 关键字：`multipleOf`
- rust 锚点：`multipleOf`→json_schema.rs — NOT IMPLEMENTED (absent keyword)
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `True` · ref accept = `False`
- 定性：缺口候选：rust 未实现 multipleOf。

### `pattern/space` — rust-reject-ref-accept
- 关键字：`pattern`
- rust 锚点：`pattern`→json_schema.rs:199-215
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `False` · ref accept = `True`
- 定性：记录性宽松差（ref 侧）：同上（pattern 编译抛错→catch→accept-all）。

### `pattern/upper` — rust-reject-ref-accept
- 关键字：`pattern`
- rust 锚点：`pattern`→json_schema.rs:199-215
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `False` · ref accept = `True`
- 定性：记录性宽松差（ref 侧，重大）：effect Draft2020-12 编译器对 pattern 默认抛错（"Pattern encountered while patterns is set to error"）→ runtime.ts Cache catch → accept-all。ref 生产路径不校验 pattern；rust（G-59 补充）执行校验。

### `prefix-items-plus-items/ok` — rust-reject-ref-accept
- 关键字：`prefixItems`, `items`
- rust 锚点：`prefixItems`→json_schema.rs — NOT IMPLEMENTED (absent keyword); `items`→json_schema.rs:349-360
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `False` · ref accept = `True`
- 定性：缺口候选（rust prefixItems 未实现的直接后果）：items 被应用到全部元素，前缀位置的 "a" 也按 {type:"integer"} 校验→误拒合法输入。

### `prefix-items/bad-first` — rust-accept-ref-reject
- 关键字：`prefixItems`
- rust 锚点：`prefixItems`→json_schema.rs — NOT IMPLEMENTED (absent keyword)
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `True` · ref accept = `False`
- 定性：缺口候选：rust 未实现 prefixItems（关键字被忽略），draft-2020-12 语义丢失。

### `prefix-items/bad-second` — rust-accept-ref-reject
- 关键字：`prefixItems`
- rust 锚点：`prefixItems`→json_schema.rs — NOT IMPLEMENTED (absent keyword)
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `True` · ref accept = `False`
- 定性：缺口候选：同上（prefixItems 未实现）。

### `type-integer/float-int` — rust-reject-ref-accept
- 关键字：`type`, `integer`
- rust 锚点：`type`→json_schema.rs:146-170 (union parse) / :31-43 (matches_type); `integer`→json_schema.rs:34-36
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `False` · ref accept = `True`
- 定性：记录性严格差：serde_json 保留 `5.0` 为 f64（is_i64=false→非 integer），JS 中 5.0===5 为整数。数字表示差，非能力缺口。

### `type-malformed-name/num` — rust-reject-ref-accept
- 关键字：`type`, `malformed-schema`
- rust 锚点：`type`→json_schema.rs:146-170 (union parse) / :31-43 (matches_type); `malformed-schema`→json_schema.rs:113 (as_object guard)
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `False` · ref accept = `True`
- 定性：畸形 schema 处置差：同上。

### `type-malformed-name/str` — rust-reject-ref-accept
- 关键字：`type`, `malformed-schema`
- rust 锚点：`type`→json_schema.rs:146-170 (union parse) / :31-43 (matches_type); `malformed-schema`→json_schema.rs:113 (as_object guard)
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `False` · ref accept = `True`
- 定性：畸形 schema 处置差：rust 将未知 type 名当作永不匹配→拒绝一切；ref 编译 codec 失败→Cache catch→accept-all。双侧都不是 spec 行为；ref 侧宽松来自 catch 通道。

### `unique-items/int-float-dup` — rust-accept-ref-reject
- 关键字：`uniqueItems`
- rust 锚点：`uniqueItems`→json_schema.rs:336-348
- ref 锚点：`packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)`
- rust accept = `True` · ref accept = `False`
- 定性：[方向与预登记不符，预登记=rust-reject-ref-accept] 语义差（记录性）：serde_json 1(u64) 与 1.0(f64) 判不相等→非重复；JS 1===1.0 判重复。与数字表示差同根因。

## 声明（宽松面，双侧预期一致）

- `format`：rust 明确不做格式断言（json_schema.rs 文档注释 :11-13）；ref 侧 effect JsonSchema 默认不启用 format 断言。corpus `format-lenient` 三对全部双 accept。
- 未知关键字：双侧忽略（rust 逐关键字提取；effect passthrough）。
- 外部 `$ref`：rust 忽略（宽松）；ref 编译失败时经 Cache catch 通道 accept-all。
- **probe 直证的 ref 生产宽松面**（effect 编译器限制，经 runtime.ts Cache catch 通道 launder 为 accept-all）：
  - `pattern`：Draft2020-12 编译器默认对 pattern 抛错（"patterns is set to error"）——ref 生产不校验 pattern，rust 校验。
  - 结构化 `const`（对象/数组值）：编译抛错（"Unsupported structured JSON Schema value for const"）——scalar const 正常。
  - `additionalProperties`（false/子 schema）：effect SchemaRepresentation 不实施——ref 生产不封闭对象。
  - 安全整数域：effect integer 拒绝 |n| ≥ 2^53；rust u64/i64 覆盖全 64 位。
  以上使对应分歧的 rust 侧（G-59/G-230 能力面）系统性**严格于** ref 生产路径——是能力超集而非缺口。

## 提取不确定性

- ref 校验语义实现位于 ref `node_modules/effect` 的 `JsonSchema.fromSchemaDraft2020_12` + `SchemaRepresentation.fromJsonSchemaDocument`（非 ref 自有代码）；本通道以 runtime.ts 的消费方式为锚，未深挖 effect 内部。
- JSON 数值表示：JS `JSON.parse` 无 int/float 之分且 2^53+1 以上丢精度；serde_json 区分 u64/i64/f64。`integer-big-int/big`（9007199254740993）两侧实际喂入的值不同（JS 侧变 9007199254740992）但判定同为 accept——列为提取噪声，非语义发现。
- 畸形 schema（type:"banana"、type:42、schema:"string"、schema:true/false）的 ref 行为取决于 effect codec 构建是否抛错；本通道镜像 runtime.ts 的 catch→accept 语义，与 effect 版本耦合。
- rust 侧 `matches_type` 对 integer 判定 `is_i64||is_u64`：负整数依赖 i64（-3 合法），u64/i64 之外的 f64 一律非 integer——`5.0` 拒绝是 serde_json 表示差，已在分歧清单定性。

## 判定

- 已裁定：152/152（每对 MATCH 或分歧方向+定性明确；未定性 0 对待人工）
- 缺口候选（rust 未实现关键字/布尔 schema）：prefixItems（含 items 全量应用的误拒副作用）、exclusiveMinimum/Maximum、multipleOf、minProperties、schema:false ——登记册候选。
- 记录性宽松差（ref 侧，probe 直证）：pattern、结构化 const、additionalProperties、|n|≥2^53 integer——effect 编译器限制经 Cache catch launder，rust 侧为能力超集。
- 记录性数字/相等语义差（5.0、1.0、[{},{}]、[1,1.0]）：JS 数值模型 vs serde_json 表示差，双侧各自自洽。
