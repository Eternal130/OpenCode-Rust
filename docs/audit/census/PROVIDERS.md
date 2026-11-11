# PROVIDERS Census（缺口普查 · provider 配置族）

> 三轴机械对拍：native entrypoint 族 / 配置键存在性 / catalog-only 家族逐项裁定；生成命令：`python3 tools/census/providers.py --write docs/audit/census/PROVIDERS.md`

## 前置锚：models.dev 快照双侧一致

- md5(rust crates/oc-core/src/provider_catalog/models-dev-snapshot.txt) = `fb458b2e999aeef6f17cad692265d9b4`
- md5(ref  /opt/opencode-ref/packages/core/src/models-dev/snapshot.txt) = `fb458b2e999aeef6f17cad692265d9b4`
- **一致 ✅** → catalog 数据面（provider id/npm/api/env 全集）双侧逐字节同源，轴 2/轴 3 的差异只能来自**路由与代码面**。

## 轴 1：native entrypoint 族全集

- ref（provider.ts `builtins` 装载表键）：**52** 个 entrypoint
- rust（facade.rs `family_of` 匹配体字面量全集）：**55** 个 entrypoint
- **BOTH：52** · REF_ONLY：**0** · RUST_ONLY：**3**
  - 🟡 RUST_ONLY `@opencode/ai/providers/amazon-bedrock/mantle/messages` — rust crates/oc-core/src/provider_catalog/facade.rs（family_of 匹配体）:556（ref builtins 装载表无 → rust-extra）
  - 🟡 RUST_ONLY `@opencode/ai/providers/anthropic-compatible` — rust crates/oc-core/src/provider_catalog/facade.rs（family_of 匹配体）:441（ref builtins 装载表无 → rust-extra）
  - 🟡 RUST_ONLY `@opencode/ai/providers/xai/chat` — rust crates/oc-core/src/provider_catalog/facade.rs（family_of 匹配体）:498（ref builtins 装载表无 → rust-extra）
- 交叉核对：rust native.rs `BUILTIN_ENTRYPOINTS`（52 项）与 ref builtins 差集：**0**（无）

## 轴 2：每家族配置键存在性（只比键，不比值）

### catalog api/env 键（快照同源 → 恒等）
- `226` 个家族中 **api（base URL）键存在：200**（rust 解析于 provider_catalog/source.rs:20 `pub api: Option<String>`；ref models-dev.ts:65/71 `readonly api?: string`）
- `226` 个家族中 **env 键存在：226**（rust source.rs env 解析 + facade.rs:1847 `package_env_chain` 补链；ref models-dev.ts:73 `readonly env: readonly string[]`）
### overlay 选项键（ref CORE_KEYS vs rust CORE_KEYS）
- ref  packages/core/src/provider.ts:142：chunkTimeout, compaction, fetch, headerTimeout, timeout, transport
- rust crates/oc-core/src/provider_catalog/facade.rs:2023：chunkTimeout, compaction, fetch, headerTimeout, timeout, transport
- **键集差异：无 ✅**
### npm→entrypoint 路由表
- PACKAGES 表差异（npm 键或映射目标不同）：**0**（无 ✅）
- HOSTS provider-id 键差异：REF_ONLY 0（无）· RUST_ONLY 0（无）

## 轴 3：catalog-only 家族逐项裁定（~180 盲区机械化）

- 快照家族总数：**226**
- **BOTH_NATIVE（专属 native entrypoint，rust family_of 覆盖）：49**
- **BOTH_COMPAT（重写到通用 `@opencode/ai/providers/openai-compatible`，双侧 native）：168** ← 上一轮「~180 catalog-only」的确凿计数
- **REF_AISDK_ONLY（专属 @ai-sdk 包，ref 走 npm 动态安装桥；rust 无移植协议 → `UnsupportedProtocol`）：9**
  - 🔴 `aihubmix`（npm `@aihubmix/ai-sdk-provider`，env ['AIHUBMIX_API_KEY']）— ref aisdk-native.ts 无映射 → provider.ts:120-138 npm 安装桥；rust facade.rs family_of None → 错误族 `UnsupportedProtocol`（facade.rs:99，登记册豁免项）
  - 🔴 `gitlab`（npm `gitlab-ai-provider`，env ['GITLAB_TOKEN']）— ref aisdk-native.ts 无映射 → provider.ts:120-138 npm 安装桥；rust facade.rs family_of None → 错误族 `UnsupportedProtocol`（facade.rs:99，登记册豁免项）
  - 🔴 `merge-gateway`（npm `merge-gateway-ai-sdk-provider`，env ['MERGE_GATEWAY_API_KEY']）— ref aisdk-native.ts 无映射 → provider.ts:120-138 npm 安装桥；rust facade.rs family_of None → 错误族 `UnsupportedProtocol`（facade.rs:99，登记册豁免项）
  - 🔴 `perplexity`（npm `@ai-sdk/perplexity`，env ['PERPLEXITY_API_KEY']）— ref aisdk-native.ts 无映射 → provider.ts:120-138 npm 安装桥；rust facade.rs family_of None → 错误族 `UnsupportedProtocol`（facade.rs:99，登记册豁免项）
  - 🔴 `qvac`（npm `@qvac/ai-sdk-provider`，env ['QVAC_API_KEY']）— ref aisdk-native.ts 无映射 → provider.ts:120-138 npm 安装桥；rust facade.rs family_of None → 错误族 `UnsupportedProtocol`（facade.rs:99，登记册豁免项）
  - 🔴 `salad-cloud`（npm `@saladtechnologies-oss/ai-sdk-provider`，env ['SALAD_CLOUD_API_KEY']）— ref aisdk-native.ts 无映射 → provider.ts:120-138 npm 安装桥；rust facade.rs family_of None → 错误族 `UnsupportedProtocol`（facade.rs:99，登记册豁免项）
  - 🔴 `sap-ai-core`（npm `@jerome-benoit/sap-ai-provider-v2`，env ['AICORE_SERVICE_KEY']）— ref aisdk-native.ts 无映射 → provider.ts:120-138 npm 安装桥；rust facade.rs family_of None → 错误族 `UnsupportedProtocol`（facade.rs:99，登记册豁免项）
  - 🔴 `v0`（npm `@ai-sdk/vercel`，env ['V0_API_KEY']）— ref aisdk-native.ts 无映射 → provider.ts:120-138 npm 安装桥；rust facade.rs family_of None → 错误族 `UnsupportedProtocol`（facade.rs:99，登记册豁免项）
  - 🔴 `watsonx`（npm `watsonx-ai-provider`，env ['WATSONX_AI_APIKEY', 'WATSONX_AI_PROJECT_ID']）— ref aisdk-native.ts 无映射 → provider.ts:120-138 npm 安装桥；rust facade.rs family_of None → 错误族 `UnsupportedProtocol`（facade.rs:99，登记册豁免项）

## 归一化与范围声明

- 轴 1 比对单位是 **entrypoint 字符串**（`@opencode/ai/providers/…`），双侧同一词汇，无归一化。
- 轴 2 只比**键存在性**（api/env/CORE_KEYS/路由表键），不比值、不比每家族默认 baseURL 字面量（ref provider 模块内 `const baseURL = …` 与 rust 路由构造的等值性由既有 parity 测试钉定，不在本通道）。
- 轴 3 的裁定以「npm→entrypoint 路由 + family_of 覆盖」为机械判据；azure/mantle 的 model-shape 特例（aisdk-native.ts:131-141 / native.rs mantle()）只影响 family 内部选路，不影响族级 BOTH 判定（声明）。
- ref 侧 `anthropic-compatible` 模块（packages/ai/src/providers/anthropic-compatible.ts 存在）不在 builtins 装载表，经 `@opencode/ai/*` 直连解析（provider.ts:106）——轴 1 以 builtins 表为权威，该差异在 RUST_ONLY 行内注明。

## Census 裁定

- 已裁定项：entrypoint 族 55/55（双侧并集逐项归属）+ catalog 家族 226/226
- 结论：轴 1 REF_ONLY 0；轴 3 REF_AISDK_ONLY 9 为缺口候选（rust 为登记册豁免的 typed-error 面）；RUST_ONLY entrypoint 3 需登记册 rust-extra 背书；BOTH_COMPAT 168 替代上一轮「~180 推定」为确凿计数。

## 验证记录

- 亲读核实（2026-11-11）：provider.ts builtins 表（48-108 行区间）逐项对照 native.rs BUILTIN_ENTRYPOINTS；aisdk-native.ts PACKAGES/HOSTS 与 native.rs packages()/host_table() 逐键对照；family_of 匹配体（facade.rs:438 起）逐字面量核对；快照 md5 双侧现算。
- RUST_ONLY 3 项人工核实：`amazon-bedrock/mantle/messages`（ref mantle() 只生成 chat/responses 两形态，aisdk-native.ts:138-141）与 `xai/chat`（ref providers/xai.ts 无 chat 子模块）为 rust 额外选路族；`anthropic-compatible` 在 ref 为独立模块但不在 builtins 表（见范围声明）。
- REF_AISDK_ONLY 项人工核实：如 `gitlab`/`perplexity` 的 npm 为专属包，aisdk-native.ts 两表均无映射，ref 经 loadPackage 的 npm.add 桥（provider.ts:120-138）支持；rust facade.rs:2151-2156 注释明示「`aisdk:` npm-install bridge is not ported」→ typed `UnsupportedProtocol`。
- 提取不确定性：(1) family_of 匹配体提取依赖 common._match_brace 的括号配平（对当前源稳定）；(2) rust HOSTS 解析按 match 臂逐臂识别 Protocols/Fixed/Many 三形态，若新增第四形态会漏；(3) 轴 3 判定不含 azure/mantle shape 特例与 copilot 同步面（copilot_sync.rs/azure_sync.rs 为 ref Console/同步面，不在 models.dev 快照内）。
- 幂等性：脚本连跑两次输出逐字节一致（/tmp/opencode/ diff 验证）。

