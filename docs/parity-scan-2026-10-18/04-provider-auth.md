# Parity Scan 04 — Provider/Model 目录、鉴权与账号（2026-10-18）

- 参考实现：`/opt/opencode-ref`（opencode v2.0.24，TypeScript）
- 目标实现：`/opt/opencode-rust`（Rust 移植）
- 方法：只读静态审计。逐文件映射 ref ↔ rust，逐流程核对（授权 URL / PKCE / 回调端口 / 轮询 / 刷新 / 过期 / 存储 / 登出 / 离线）。所有差异给出双方 `path:line`。
- 严重级：**S0**＝主干功能断裂；**S1**＝功能缺失或行为错误；**S2**＝边缘/UX/文案。
- 状态标注：`MISSING / STUB / PARTIAL / WRONG / EXTRA / OK`；已知问题对照 `ROADMAP.md`、`docs/be3.log`、`docs/be3b.log`、`docs/audit*.md` 标 `KNOWN`，其余为 `NEW`。

---

## 0. 总览

**总体结论**：目录数据面（models.dev 解析/规范化/variant 协议表/成本/缓存刷新/离线回退）与集成鉴权面（integration host、credential 存储、10 个 OAuth/命令登录流程、CLI `auth *`、well-known 发现）移植得**相当完整且高质量**（大量逐行引用 v2 行号并有测试锁定）。**但「登录得到的凭据 → 模型解析 → 请求鉴权」这最后一公里没有打通到 v2 的通用程度**：只有 copilot/vertex/console 三个特例接进了解析器，其余 provider 的存储凭据既不进入路由鉴权，也不进入 `/api/model` 可用性折叠。另有若干 NEW 级差异（variant 请求落地、config 对 catalog provider 的覆盖、Poe client id、超时不生效等）。

统计：

| 级别 | 数量 | 代表 |
|---|---|---|
| S0 | 3 | 存储凭据不进入解析/鉴权；console 颁发 provider 无 token；keyless zen 请求鉴权失败（静态推断） |
| S1 | 9 | variant overlay 未落地、config 覆盖丢失/错绑、Poe 登录损坏、chatgpt/codex provider 整形缺失、超时不生效等 |
| S2 | 10 | JWKS 验签（KNOWN）、hostID、method 迁移、本地发现、CLI 交互提示等 |

协议族覆盖（按 pinned snapshot 226 providers 计算，脚本复刻 `native.rs` 解析 + `facade.rs::family_of`）：**217/226 个 provider 可绑定到已移植协议族；9 个完全不可绑**（`perplexity`、`gitlab`（AI 网关）、`salad-cloud`、`watsonx`、`aihubmix`、`merge-gateway`、`v0`、`sap-ai-core`、`qvac` —— 均为 `aisdk:` shim，选中时得到干净的 `UnsupportedProtocol` 类型化错误，见 `facade.rs:697-732` 测试）。无「部分模型可绑」的混合 provider。

---

## 1. 架构概念映射

| v2 概念 | ref 位置 | Rust 对应 | 状态 |
|---|---|---|---|
| Provider.Service（可变注册表 + 可用性折叠 + 事件） | `packages/core/src/provider.ts:282-449` | 静态 `Catalog`（`provider_catalog/catalog.rs`）+ 引擎快照；无 per-location 可变 Editor/事件 | PARTIAL（架构性简化） |
| Model.Service（默认/小模型/合并 overlay） | `model.ts:88-283` | `oc-schema/model.rs` + `engine/resolver.rs` + `session/runner/context.rs:1327-1349`（small） | PARTIAL |
| ModelResolver（variant/凭据/包加载） | `model-resolver.ts` | `engine/resolver.rs` + `provider_catalog/facade.rs` | PARTIAL（见 A-3/A-4/B-1） |
| models.dev 加载/缓存/刷新 | `models-dev.ts` | `provider_catalog/{catalog,refresh,source,support}.rs` | OK |
| variant 协议表 | `variant.ts:566-621` | `provider_catalog/variant.rs`（全表移植） | OK（目录侧）；请求侧 MISSING |
| Integration host（OAuth/命令尝试、凭据解析、状态） | `integration.ts` | `oc-core/src/integration/mod.rs`（1180 行） | OK |
| Credential 服务（DB 多账号） | `credential.ts` + `credential/sql.ts` | `integration/credential.rs` + `database/migrations.rs:402-415` | OK |
| OAuth 流程插件（console/copilot/xai/openai/chatgpt/gitlab/azure/snowflake/do/poe） | `plugin/provider/*.ts` | `integration/providers.rs` + `providers_enterprise.rs` | PARTIAL（见 D 节） |
| well-known 发现 + 配置图 | `wellknown.ts` + `wellknown/plugin.ts` + `config.ts:144-170` | `oc-server/handlers/integration.rs:141-297` + `integration/wellknown_config.rs` | OK |
| npm provider 包运行时加载 | `provider.ts:112-139` | 无（`UnsupportedProtocol` 清晰错误） | MISSING（KNOWN/设计取舍） |
| route Auth 组合子 | `packages/ai/src/route/auth.ts` | `oc-ai/src/route/auth.rs`（453 行全组合子） | OK |
| OAuth 回调 HTML 页面 | `oauth/page.ts`（品牌化） | `integration/flows.rs:204-229`（简化页） | PARTIAL（S2） |
| account 表（legacy） | `account/sql.ts:5-36` | `database/migrations.rs:140-183`（含 active_org_id 迁移） | OK（双方均仅保留） |

---

## 2. 文件级映射与缺失清单

### 2.1 参考侧 → Rust 侧

| ref 文件 | rust 对应 | 状态 | 备注 |
|---|---|---|---|
| `packages/core/src/provider.ts` | `provider_catalog/facade.rs`（绑定半）+ `native.rs`（aisdk-native 表）+ `engine/resolver.rs` | PARTIAL | npm 加载 MISSING（KNOWN）；timeout DEFAULT 300s 未执行（B-6）；无 Provider.State/事件流（架构性） |
| `packages/core/src/model.ts` | `oc-schema/src/model.rs` + `engine/resolver.rs` + `session/runner/context.rs` | PARTIAL | mergeCapabilities 未用于 console/catalog 合并（be3b KNOWN）；default 的 enabled 检查缺失（A-7） |
| `packages/core/src/model-resolver.ts` | `engine/resolver.rs` + `facade.rs` | PARTIAL | variant overlay（A-3）、凭据解析（B-1）、`${VAR}` baseURL 替换（A-6）、Auth.none 规则（B-3） |
| `packages/core/src/variant.ts` | `provider_catalog/variant.rs`（1304 行） | OK | 全协议表移植；预算钳制/去重/claude 解析均带测试 |
| `packages/core/src/models-dev.ts` | `provider_catalog/catalog.rs` + `refresh.rs` + `source.rs` + `support.rs` | OK | normalize/cost/mergeCost/modes/digest/TTL/digest-skip/离线回退齐全 |
| `packages/core/src/models-dev/snapshot.txt` | `provider_catalog/models-dev-snapshot.txt`（226 providers / 8495 models） | OK | pinned 一致性有测试（`catalog.rs:489-496`） |
| `packages/core/src/credential.ts` | `integration/credential.rs` | OK | 排序/激活/替换提升/事件逐条对齐 |
| `packages/core/src/credential/sql.ts` | `database/migrations.rs:402-415` | OK | 含 legacy `auth.json` 导入（`migrations.rs:470+`，ref `20260805200742` 同名迁移） |
| `packages/core/src/account/sql.ts` | `database/migrations.rs:140-183` | OK(EXTRA) | 双方都只是保留 legacy 表 |
| `packages/core/src/integration.ts` | `integration/mod.rs` | OK | 尝试生命周期/settle/scrub/表单校验齐全 |
| `packages/core/src/integration/connection.ts` | `integration/types.rs:84-117` | OK | connection key 语义一致 |
| `packages/core/src/wellknown.ts` | `oc-server/handlers/integration.rs:141-297` + `integration/wellknown_config.rs` | PARTIAL | `wellknown.remove` 无 HTTP 面（v2 也未暴露，等价）；`WellKnown.refresh()` 增量对比未做（每次全量重取，行为近似） |
| `packages/core/src/wellknown/plugin.ts` | `handlers/integration.rs:242-297`（`ensure_wellknown_registered`） | PARTIAL | v2 在 boot 即注册；Rust 在首次 `integration.list` 时惰性恢复（`integration.rs:107`）——首次 `auth login`（不经 list）仍会经 `load_integrations` 触发，等价 |
| `packages/core/src/oauth/page.ts` | `integration/flows.rs:200-229` | PARTIAL | 简化 HTML（无品牌/错误详情页/auto-close 时序差异小） |
| `packages/core/src/github-copilot/copilot-provider.ts` | `oc-ai/src/protocols/copilot.rs`（copilot_chat_route） | OK | |
| `packages/core/src/github-copilot/models.ts`（动态模型同步） | **无** | MISSING | KNOWN（be3b.log Phase 3 deferred） |
| `packages/core/src/plugin/provider/opencode.ts` | `integration/providers.rs:74-248` + `console.rs` | PARTIAL | keyless zen 请求侧（B-3）；console token（B-2）；per-model 合并（KNOWN） |
| `packages/core/src/plugin/provider/openai.ts` | `providers_enterprise.rs:826-1102` | PARTIAL | 登录流 OK；provider 整形 MISSING（D-6） |
| `packages/core/src/plugin/provider/chatgpt.ts` | `providers_enterprise.rs:1113-1324` | PARTIAL | JWKS KNOWN；hostID/savedClientID/migration MISSING（D-4/D-5） |
| `packages/core/src/plugin/provider/xai.ts` | `providers.rs:476-584` | PARTIAL | browser→device 迁移、错误文案、JWT exp 回退、websocket 默认（D-7） |
| `packages/core/src/plugin/provider/gitlab.ts` | `providers_enterprise.rs:80-360` | OK | 固定 8080/S256/单飞刷新/提示语均对齐 |
| `packages/core/src/plugin/provider/azure.ts` | `providers_enterprise.rs:365-585` | PARTIAL | key 方法表单恒显示（v2 仅未配置时）——S2 |
| `packages/core/src/plugin/provider/snowflake-cortex.ts` | `providers_enterprise.rs:591-820` | OK | 48 字节 PKCE / session:role[-encoded] / Basic LOCAL_APPLICATION / env-token 跳过刷新 |
| `packages/core/src/plugin/provider/digitalocean.ts` | `providers.rs:599-670` | OK | 1456 端口 / implicit bootstrap / state |
| `packages/core/src/plugin/provider/poe.ts` | `providers.rs:676-795` | **WRONG** | client id 与 token 字段错（D-8） |
| `packages/core/src/plugin/provider/{ollama,lmstudio,vllm,zenmux,kilo,modal,perplexity,llmgateway,sap-ai-core,nvidia,…}.ts` | **无**（仅 catalog 静态条目） | MISSING | 本地动态发现（A-8） |
| `packages/core/src/plugin/provider/{amazon-bedrock,cloudflare-workers-ai,google-vertex,cloudflare-ai-gateway,openrouter,cohere,…}.ts`（provider/model transform 类） | 部分：bedrock 激活/vertex project 注入 **无**；env 方法过滤不全 | PARTIAL | 见 A-5 |
| `packages/core/src/plugin/models-dev.ts`（注册/过滤） | `integration/providers.rs:41-68` | PARTIAL | azure 过滤有；vertex/bedrock/cloudflare 过滤无（A-5）；bedrock profile-only ID 过滤无（A-9） |
| `packages/cli/src/commands/handlers/auth/*`（8 文件 841 行） | `oc-cli/src/handlers/auth.rs`（890 行） | PARTIAL | 交互选择器/URL 打开/表单交互提示（G 节） |
| `packages/ai/src/provider-package.ts` + `providers/*` 注册元数据 | `oc-ai/src/protocols/*`（协议实现即注册） | PARTIAL | npm Settings 契约由 `ProviderSettings.rest` 近似 |
| `packages/ai/src/route/auth.ts` | `oc-ai/src/route/auth.rs` | OK | 组合子/缺失报错语义一致 |

### 2.2 Rust 侧文件覆盖（本子系统）

| rust 文件 | 行数 | 对应 ref | 覆盖度 |
|---|---|---|---|
| `oc-core/src/provider_catalog/catalog.rs` | 740 | models-dev.ts normalize | 高（含 pinned 测试） |
| `oc-core/src/provider_catalog/refresh.rs` | 595 | models-dev.ts fetch/cache | 高 |
| `oc-core/src/provider_catalog/native.rs` | 273 | aisdk-native.ts | 高 |
| `oc-core/src/provider_catalog/variant.rs` | 1304 | variant.ts | 高 |
| `oc-core/src/provider_catalog/{source,support}.rs` | 264 | models-dev.ts 源类型 | 高 |
| `oc-core/src/provider_catalog/overlay.rs` | 290 | config→provider | 中（A-4） |
| `oc-core/src/provider_catalog/facade.rs` | 997 | provider.ts 绑定半 | 中高 |
| `oc-core/src/engine/resolver.rs` | 436+ | model-resolver.ts | 中（A-3/B-1） |
| `oc-core/src/integration/mod.rs` | 1180 | integration.ts | 高 |
| `oc-core/src/integration/credential.rs` | 397 | credential.ts | 高 |
| `oc-core/src/integration/providers.rs` | 795 | opencode/copilot/xai/do/poe 插件 | 高（Poe 字段错） |
| `oc-core/src/integration/providers_enterprise.rs` | 1324 | gitlab/azure/snowflake/openai/chatgpt | 高 |
| `oc-core/src/integration/{flows,copilot,console,vertex,wellknown_config,types}.rs` | 1476 | flows/绑定/console/ADC/wellknown | 高 |
| `oc-ai/src/route/auth.rs` | 453 | route/auth.ts | 高 |
| `oc-server/src/handlers/{integration,credential,model}.rs` | 1564 | handlers/* | 高 |
| `oc-cli/src/handlers/auth.rs` | 890 | handlers/auth/* | 中高 |

---

## 3. 差异明细

### A. Provider/Model 目录与解析

#### A-1【S1·NEW】catalog 刷新后解析面与集成注册面不更新（需重启）
- EXPECTED（ref）：`models-dev.refreshed` → `ModelsDevPlugin` 重新执行 integration transform + provider transform（`plugin/models-dev.ts:61-74`）；所有 Resolver 读 `ModelsDev.get()`（invalidate 后的新目录）。
- ACTUAL（rust）：刷新只更新 `catalog_service`（`refresh.rs:251`）；`Engine.catalog`（boot 时 `catalog_service.get()` 的冻结 Arc，`engine/mod.rs:427`）被 location resolver 继续使用（`engine/mod.rs:673`），session runner 的 `ContextDeps.catalog` 同源；`providers::install` 只在 boot 用 boot 目录跑一次（`engine/mod.rs:455-460`），新 provider 的 env/key 方法不注册。
- 影响：`/api/model`、`/api/provider` 能看到新目录（读 `catalog_service.get()`，`handlers/model.rs:91`），但新 provider/model **无法被会话解析**、新 provider 无法 `auth login`，直到重启进程。5 分钟 TTL 的意义减半。
- 级别：S1（长驻 daemon 场景）。

#### A-2【OK】模型 id / ref 规范化
- `Model.parse`（ref `model.ts:298-304`，首个 `/` 切分）↔ `oc-schema/src/model.rs:29-54`（`providerID/modelID#variant`，拒绝空 variant/多 `#`）。CLI `-m`/mini `parse_model`（`oc-cli/src/mini.rs:104`）一致。模式条目 `${model.id}-${mode}` 与 `modelID` 双字段（`catalog.rs:226-239` ↔ `models-dev.ts:117-127`）一致。

#### A-3【S1·NEW】variant（reasoning effort / thinking budget）overlay 未应用于请求
- EXPECTED（ref）：`withVariant` 把选中 variant 的 `settings/headers/body` 合并进 runtime model（`model-resolver.ts:137-161`），再进包 settings → provider options → 线上请求体（`resolveCatalogModel` `model-resolver.ts:196-257`）。`claude-opus-4-5#max` → `thinking.budgetTokens`；`gpt-5.4#xhigh` → `reasoningEffort`。
- ACTUAL（rust）：variant 目录生成齐全（`variant.rs`），ref 记录/展示/`model_switched` effort 标记（`session/runner/to_llm_message.rs:278-308`）齐全，但 **没有任何代码把 `reference.variant` 对应的 overlay 并入 route defaults 或 LLMRequest**：`engine/resolver.rs:154-261 bind()` 不读 variant；`facade.rs:515-535 apply_model_overlays` 只用 model 级 settings；`session/runner/model_request.rs:574-607` 只合并 hook 选项。resolver 只在 variant 为空时补 `"default"`（`resolver.rs:371-375`，v2 中 `"default"` 是 no-op，等价）。
- 影响：用户选择 `#high/#max/#thinking` 后请求体不变——reasoning/thinking 全部不生效（目录侧白做）。TUI 模型对话框的 variant 子选择（ROADMAP wave B #11）实际无效。
- 级别：S1（核心交互缺失）。可能与请求路径 parity 报告重叠，此处按「variant/reasoning 解析」归入本域。

#### A-4【S1·NEW】config 对 catalog provider 的 settings 覆盖丢失 / 错绑
- EXPECTED（ref）：`config/plugin/provider.ts` 把 `provider.<id>.settings/headers/body/models` **合并到 catalog provider 之上**（保留原协议包；`configured.ts:14-18` foldSettings；`provider.ts:172-205` mergeOverlay/Headers）。`providers.anthropic.settings.apiKey` → 请求鉴权；`providers.anthropic.settings.baseURL`（企业代理）→ anthropic messages 协议换端点。
- ACTUAL（rust）：`provider_catalog/overlay.rs:38-117` 只把「有 baseURL 的 `providers.<id>`」变成 **openai-compatible CustomProvider**：① catalog provider 设 baseURL → 被整体重绑为 openai-compatible chat（协议错误）；② 只设 `apiKey`（无 baseURL）→ **整条被跳过**（`overlay.rs:276-289` 测试显式断言 skip），鉴权不生效；③ `providers.<id>.package/canonical` 字段（`oc-schema/src/config/provider.rs:172-189` 已建模）被忽略。`facade.rs:108 ModelConnection::from_settings` 为死代码（全仓无调用）。
- 影响：v1/v2 最常见的「在 opencode.json 里给 anthropic/openai 填 key 或代理地址」在 Rust 端要么丢 key、要么协议错绑。
- 级别：S1。

#### A-5【S1·NEW】catalog env 方法过滤不全（vertex / bedrock / cloudflare-workers-ai）
- EXPECTED（ref）：`plugin/models-dev.ts:77-87 environmentNames`：`google-vertex` → 仅 `GOOGLE_VERTEX_API_KEY`（project/location/ADC 不是 key）；`cloudflare-workers-ai` → `[CLOUDFLARE_API_KEY, CLOUDFLARE_WORKERS_AI_TOKEN, CLOUDFLAKE_API_TOKEN]`；另 `plugin/provider/amazon-bedrock.ts:22-30` 把 bedrock env 方法**替换**为仅 `AWS_BEARER_TOKEN_BEDROCK`（SigV4 变量不得成为 key 凭据）。
- ACTUAL（rust）：`integration/providers.rs:54-65` 只特判 `azure`。snapshot 中 `google-vertex.env = [GOOGLE_VERTEX_PROJECT, GOOGLE_VERTEX_LOCATION, GOOGLE_APPLICATION_CREDENTIALS]`、`amazon-bedrock.env = [AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION, AWS_BEARER_TOKEN_BEDROCK]`、`cloudflare-workers-ai.env = [CLOUDFLARE_ACCOUNT_ID, CLOUDFLARE_API_KEY]` 原样成为 env 方法。
- 影响：装了 gcloud/AWS CLI 的机器上，这些 provider 被误判为「已连接」→ `/api/model` 出现全套模型（`handlers/model.rs:92-93`）、`auth list` 显示 env 连接；bedrock 还会拿 `AWS_ACCESS_KEY_ID` 当 bearer。同时 v2 的 bedrock SigV4-链激活（`amazon-bedrock.ts:31-45` `activation=enabled`）与 vertex project/location 注入+激活（`google-vertex.ts:44-69`）均无对应。
- 级别：S1。

#### A-6【S2·NEW】`${VAR}` baseURL 环境变量替换与校验缺失
- EXPECTED（ref）：`model-resolver.ts:280-306` —— settings.baseURL 中的 `${NAME}` 用 `process.env` 替换，无法解析则 `UnresolvedProviderVariablesError`（`X, Y are required to resolve the provider endpoint`）。
- ACTUAL（rust）：`facade.rs:271` 直接使用字面 baseURL；无替换、无校验错误。
- 影响：模板式 baseURL（azure/vertex 风格配置）发送到非法 URL。
- 级别：S2（配置面较罕见；vertex 自有 project 注入另计）。

#### A-7【S2·NEW】`model.default` 不校验 enabled/可用性
- EXPECTED（ref）：`model.ts:260-265` —— 配置的默认模型必须在 **available**（enabled）集合内，否则回退 `available.find(supportsText)`。
- ACTUAL（rust）：`oc-server/handlers/model.rs:173-185` 直接在 `catalog.providers()` 里查（不看 enabled / 凭据可用性），查到即返回；session 侧 `resolver.rs:264-279` 同样先 pin 后失败（ModelError 而非回退）。
- 影响：配置了不可用默认模型时，v2 静默回退首个可用模型，Rust 报 ModelUnavailable。
- 级别：S2。

#### A-8【S2·NEW】本地 provider 动态发现缺失（ollama / lmstudio / vllm / zenmux / kilo / modal …）
- EXPECTED（ref）：`plugin/provider/local.ts` 工厂 —— 每 30s 发现 `/api/tags` 等，注册 `activation:"enabled"` 的 openai-compatible provider + 动态模型（tools/vision/context 推断），断网保留上次清单。
- ACTUAL（rust）：无对应；这些 provider 仅剩 catalog 静态条目（env 未设时不可用）。
- 级别：S2（本地推理用户为 S1，但基础路径仍可用 env+catalog）。

#### A-9【S2·NEW】bedrock profile-only 模型与 deprecated 别名 provider 未过滤
- EXPECTED（ref）：`plugin/models-dev.ts:11-25,94-110` —— 13 个 bedrock inference-profile-only 模型 ID 不注册；`azure-cognitive-services`、`google-vertex-anthropic` 两个别名 provider 整体剔除。
- ACTUAL（rust）：`catalog.rs:137-165 available_models` 只过滤 deprecated + 非 text；两个别名 provider 仍在目录（snapshot 各 1 个）；bedrock profile-only 模型照常列出。
- 影响：列表出现 v2 不展示的模型/provider（config 层有 id 重定向 `config/normalize.rs:80-81,2105-2108`，仅部分缓解）。
- 级别：S2。

#### A-10【EXTRA·故意的容差】resolver 大小写不敏感回退
- `engine/resolver.rs:124-148,166-178`：catalog 前缀与自定义 provider/model 键先精确后忽略大小写；v2 全部精确匹配（`model.ts:255-257`）。代码内注明是 beta.3.1 缺陷的容差。EXTRA（行为超集，非缺陷）。

### B. 鉴权应用（credential → 请求）

#### B-1【S0·NEW】存储凭据不进入模型解析与请求鉴权（除 copilot/vertex）
- EXPECTED（ref）：`ModelResolver.load` 对**每个** provider 取 `integrations.connection.active(provider.integrationID ?? providerID)` 并 `resolve` 出凭据（`model-resolver.ts:364-371`）；key → `apiKey`，OAuth → `access`（`nativeCredentialSettings` `model-resolver.ts:308-319`）。`auth login <provider>` 存 key 后该 provider 立即可用。可用性折叠同样吃 integration connections（credential + env，`provider.ts:393-406`、`integration.ts:369-383`）。
- ACTUAL（rust）：`CatalogModelResolver` 的连接来源只有 ① boot/global 的 `EngineOptions.connections`（生产 boot 从不填充，`boot.rs:159-181` 未触及）② location config 生成的 CustomProvider（A-4）③ `CredentialBindings`（**仅** `github-copilot` 与 `google-vertex`，`integration/copilot.rs:108-119`）④ `ConsoleProviders`（无 token，见 B-2）。`auth login anthropic`（key）/`auth login xai`（SuperGrok OAuth）等存储的凭据：不进入 `facade.rs:298-304 bearer_auth`（回退 catalog env 链 → `Missing auth credential`），也不进入 `/api/model` 可用性（`handlers/model.rs:92-93` 只查 env var + keyless zen）。
- 影响：**登录流程完整可用但登录后绝大多数 provider 仍发不出请求**；模型列表也不出现。be3b 已为 copilot/vertex 建了绑定机制，通用化未做（ROADMAP「integration/auth host lane — unlocks /connect」暗示意图，但未声明完成）。
- 级别：S0（子系统主链路）。

#### B-2【S0·NEW】console 颁发 provider 不携带 token
- EXPECTED（ref）：console provider 合并时设 `provider.integrationID = "opencode"`（`opencode.ts:251`），`ModelResolver` 解析 opencode 集成凭据 → OAuth access 作为 apiKey 传入（`model-resolver.ts:369-371,308-319`）；settings 内凭据被剥除但凭据在解析期注入（`opencode.ts:547-552`）。
- ACTUAL（rust）：`integration/console.rs:180-217 to_custom_providers` 投影出的 `CustomProvider.api_key = None`（`console.rs:210`）→ `facade.rs:569-576` 走 `auth::Definition::none()` 匿名请求。
- 影响：console（zen 组织）登录后，console 颁发的 provider 请求全部未鉴权（401）。be3b Phase 4 声明「fetch → status → bind 完成」，但 token 绑定不在其中 → NEW。
- 级别：S0。

#### B-3【S0·NEW（静态推断，建议运行时复现）】keyless zen 免费层请求鉴权失败
- EXPECTED（ref）：无 `OPENCODE_API_KEY`/连接时 zen provider `activation=enabled` 且 `settings.apiKey="public"`（`opencode.ts:317-325`），付费模型禁用（`:327-342`）；请求带 `Bearer public`（或按 `model-resolver.ts:379-385` 对 enabled+无凭据+API-key 包降级 `Auth.none`）。
- ACTUAL（rust）：可用性侧对齐（`catalog.rs:137-165`，keyless 只列零成本模型，测试锁定），但解析侧无任何 `"public"` 注入：`opencode` provider（`@ai-sdk/openai-compatible`，baseURL `https://opencode.ai/zen/v1`）经 `facade.rs:476-485` → `bearer_auth(None, ["OPENCODE_API_KEY"])` → `auth.rs:101-112` 空值/未设 env 均为 Err → 请求期 `AiError::Authentication`。
- 影响：全新安装无 key 无登录时，默认模型（zen 免费层最新模型）首次发送即鉴权错误。目录/列表一切正常，属于「最后一跳」断裂。仓库内无 keyless zen 的请求路径测试（仅目录测试）。
- 级别：S0（开箱体验主干；因未运行时复现，标注待验证）。

#### B-4【OK】oc-ai route auth 组合子
`oc-ai/src/route/auth.rs` 对 `route/auth.ts` 的 `value/optional/config/none/headers/remove/andThen/orElse/bearer/header` 逐项移植，缺失报错语义（`X is not set` / `Missing auth credential: value`）一致（`auth.rs:59-113` ↔ ref `auth.ts:11-21,53-70`）。`bearer_from_options`（`auth.rs:314-325`）覆盖 `AuthOptions.bearer` 优先级。

#### B-5【OK】Copilot 请求头策略（基础集）
`copilot.rs:57-93`：`X-GitHub-Api-Version: 2026-08-01`、`Openai-Intent: conversation-edits`、`x-initiator`、apiEndpoint > enterprise > 默认 base URL，与 `github-copilot.ts:359-377,464-505` 一致；`X-GitHub-Api-Version` 线上断言见 `facade.rs:856`。**缺**：每请求 `x-initiator` 升级 / `Copilot-Vision-Request` / `X-Interaction-Type`（ref `github-copilot.ts:266-316,500-554`）——KNOWN（be3b Phase 3 deferred）。

#### B-6【S1·NEW】`timeout/headerTimeout/chunkTimeout` 不生效
- EXPECTED（ref）：`provider.ts:142-170` —— provider settings 的三个超时并入路由 HTTP 默认；`headerTimeout/chunkTimeout` 缺省 **300s**。
- ACTUAL（rust）：`oc-ai/src/schema/options.rs:130,148-183` 定义并合并 `HttpTimeout`，但全仓无执行点：`route/transport.rs:211-235` 注释「Timeouts are the caller's concern」且 `ReqwestTransport` 无超时设置，route/executor 无 `tokio::time::timeout` 包裹。
- 影响：停滞的流式响应永不超时（v2 5 分钟必断）。
- 级别：S1。

### C. models.dev 数据加载与缓存刷新（离线行为）

#### C-1【OK】加载优先级与缓存
- ref `models-dev.ts:385-401 populate`：file → KV cache → bundled snapshot → fetch；Rust `refresh.rs:190-205 populate_from_cache` + `adopt`（file/snapshot）+ 引擎 boot 用 options.catalog（快照）→ 首次 refresh。缓存损坏即删除（`refresh.rs:196-204` ↔ ref `:342`）。
- KV：键 `models-dev:catalog`（`refresh.rs:26` ↔ ref `:300`），条目 `{updatedAt, digest, body}`（`refresh.rs:245-250` ↔ ref `:272-278,369-376`）。
- TTL 5min、digest-skip、decode-before-write、`models-dev.refreshed` 事件（digest 变化才发）逐项对齐（`refresh.rs:228-265` ↔ ref `:407-429`；测试 `refresh.rs:359-394`）。
- 离线：fetch 失败保留快照（`refresh.rs:579-594` 测试 ↔ ref log-and-ignore `:426-427`）。
- 差集：ref 对非默认 source 用 `models-dev:catalog:{hash}` 键（`:299-302`）；Rust `CatalogService` 只有默认键（`CACHE_KEY` 常量，自定义 URL 也写默认键）。S2（多源场景罕见）。Rust `OPENCODE_MODELS_DEV_URL`/`OPENCODE_CATALOG_FETCH` 环境开关为 EXTRA（`boot.rs:165-181`）。

#### C-2【OK】规范化细节
normalize/cost（零填充+tiers）/mergeCost（tier 键合并）/released 日期（UTC 午夜，坏值 0）/modalities/tools/status/enabled/limit.input/cloudflare-workers-ai baseURL 例外/openai-compatible `provider` 注入 —— `catalog.rs:170-473` ↔ `models-dev.ts:85-250`，均有 pinned 测试（226/8495、qwen3.5-flash 变体、fireworks baseURL 等）。`supports` 的 null/"null" 过滤（`support.rs:29-49` ↔ ref `:196-202`）一致。

### D. OAuth 流程逐项

通用机制（尝试生命周期 10min/终态保留 1min/30s scrub/auto 模式 connect 即 fork/code 模式 complete 驱动/cancel/persisting 保护/表单校验 required+when+pattern）`integration/mod.rs:34-39,574-805,1042-1180` ↔ `integration.ts:237-239,558-608,762-795,509-556`：OK。刷新窗口（过期前 5min）与刷新写回（`mod.rs:431-466` ↔ `integration.ts:696-714`）：OK。

| 流程 | ref | rust | 结论 |
|---|---|---|---|
| OpenCode Console device | `opencode.ts:88-165`（client `opencode-cli`、`supports_org_scope`、verification_uri_complete 相对解析、orgID/orgName label、invalid_grant→重连提示） | `providers.rs:74-248` | **OK**（细节对齐：`:126-144` 相对 URL 三态、`:208-213` invalid_grant 文案、`:219-230` 刷新保留 orgName）。差异：Rust 用 device.expires_in 设尝试过期（ref 留默认 10min）——可忽略；form 字段无 `format:"uri"` |
| GitHub Copilot device | `github-copilot.ts:47-157`（表单 deploymentType/enterpriseUrl、entitlement 检查、`expires:0`、enterprise/apiEndpoint metadata） | `providers.rs:259-470` | **OK**（`:358-371` entitlement：显式 false 拒绝，失败放行；`:448-454` `expires:0`；`:318` 移除 key 方法）。EXTRA：`copilot_domain` 保留显式 `http(s)://` origin（ref 剥 scheme）——自托管/测试便利。**注意**：Rust entitlement 文案与 ref 不同（ref 引导注册 Copilot Free，`github-copilot.ts:365-370`；Rust 简单一句话）且未读 `can_signup_for_limited`——S2 文案差 |
| xAI device | `xai.ts:40-74,125-168` | `providers.rs:476-584` | **PARTIAL**：参数/URL/说明文案一致；缺 ①旧 `browser` 凭据迁移到 `device`（ref `:80-89`）②`access_denied/expired_token` 专属文案（ref `:155-160`）③无 expires_in 时从 JWT exp 回退（ref `:214-223`）④provider 默认 `transport:"websocket"`（ref `:98-105`；Rust 无 websocket lane，`route/client.rs:2` 注明） |
| OpenAI Codex browser | `openai.ts:52-109,111-169`（1455→1457、`/cancel` 探测×10×200ms、PKCE 43 字符、authorize 参数含 originator/codex_cli_simplified_flow、accountID claims） | `providers_enterprise.rs:928-988` + `flows.rs:80-129` | **OK**（`listen_pinned` 复刻取消探测与双端口；PKCE 43 字节 `:1099-1102`；claims 三级回退 `:882-908`） |
| OpenAI Codex headless | `openai.ts:171-230` | `providers_enterprise.rs:990-1074` | **OK**（usercode→poll 403/404→服务端 verifier→`{issuer}/deviceauth/callback` 兑换，`+3s` 余量） |
| ChatGPT token-sharing | `chatgpt.ts:91-192,498-519,439-459` | `providers_enterprise.rs:1113-1324` | **PARTIAL**：OK——scope 强制、id_token 必需、iss/aud/exp/nonce/sub 检查、clientID/scopes metadata、刷新需 clientID。MISSING——①RS256/JWKS 验签（**KNOWN**，be3b.log 明示 deferred）②持久 `chatgpt-agent-host-id` + `ext_agent_host_id`（ref `:100-107,512`）③已注册 clientID 复用（ref `:166-172` 回调可省 client_id；Rust `:1201-1204` 强制要求）④旧 `chatgpt-browser` 凭据迁移（ref `:237-256`）⑤retry 钩子对不可重试 sharing 错误码的抑制（ref `:258-266`） |
| GitLab PKCE | `gitlab.ts`（固定 8080、bundled/legacy/GITLAB_OAUTH_CLIENT_ID、非 gitlab.com 拒绝、刷新单飞、invalid_grant 提示） | `providers_enterprise.rs:80-360` | **OK**（`:245-250` 非 gitlab.com 拒绝文案、`:310-334` keyed mutex 单飞、`:194-209` 提示语） |
| Azure Entra CLI | `azure.ts:88-166` | `providers_enterprise.rs:365-585` | **OK/PARTIAL**：`az` 存在才注册（`:388-390`）、10s 超时、`az login` 提示、expires_on/expiresOn(RFC-1123) 解析、refresh 重跑 CLI。差异：key 方法的 resourceName 表单 v2 仅在「未配置」时展示，Rust 恒展示——S2 |
| Snowflake external browser | `snowflake-cortex.ts` | `providers_enterprise.rs:591-820` | **OK**（48 字节 PKCE、`session:role[-encoded]`、Basic `LOCAL_APPLICATION`、account 归一化、env-token 跳过刷新、label=account） |
| DigitalOcean implicit | `digitalocean.ts` | `providers.rs:599-670` | **OK**（1456、bootstrap 页 POST-back、state、30d 默认过期） |
| **Poe** | `poe.ts:10-16,128-163`：client **`client_728290227fc048cc9262091a1ea197ea`**；token 响应读 **`api_key` / `api_key_expires_in`**（≤100 天封顶） | `providers.rs:676-772`：client **`9rKyfhAcBR0rEmvPpKcVxpDzqEBzZPPK`**；读 **`access_token` / `expires_in`** | **WRONG（S1）**：client id 与 v2 不符（Poe 会拒绝该 client 的授权请求），且兑换响应字段名错（即使拿到 code 也解析失败）→ Poe 登录端到端损坏。次差：无 `iss` 校验、无错误详情泄密防护、过期无 `maxExpiry` 封顶（ref `:117-160`） |
| well-known command login | `wellknown/plugin.ts` + CLI | `handlers/integration.rs:141-297` | **OK**（manifest auth.command 必需校验、hostname 命名、`login` 命令方法、kv 持久化+惰性恢复） |
| MCP OAuth | ref `mcp/oauth.ts` | `oc-core/src/mcp/*`（有 oauth） | 超出本报告范围（另见 MCP 报告） |

#### D-6【S1·NEW】ChatGPT/Codex 登录后的 provider 整形完全缺失
- EXPECTED（ref）：`openai.ts:256-299` —— ChatGPT OAuth 存在时 openai provider：`baseURL=chatgpt.com/backend-api/codex`、默认 websocket transport、`originator/x-codex-beta-features/chatgpt-account-id` 头、codex 模型白/黑名单过滤、成本清零、`limit.context=400k/input=272k`、session `session-id` 头、context/compaction 去 maxTokens；`chatgpt.ts:267-306` —— token-sharing 时 `baseURL=api.openai.com/v1`、summary compaction、fallbackModels 过滤、成本清零、pro 模式禁用。
- ACTUAL（rust）：登录方法存在（providers_enterprise），但 resolver/facade 对 `openai` provider 无任何凭据驱动的整形（无 codex baseURL、无模型过滤、无成本清零；grep 全仓无 `backend-api/codex` 生产代码）。
- 影响：用 ChatGPT 订阅登录后，请求仍打 `api.openai.com` 且用订阅不支持的方式（模型不过滤、native compaction 不禁）。
- 级别：S1。

### E. Credential 存储 / 多账号 / 导入导出 / 登出

| 项 | ref | rust | 结论 |
|---|---|---|---|
| 存储位置/格式 | SQLite `credential` 表，`value` 列 JSON（`credential/sql.ts:5-14`；全局 DB） | 同表同列（`migrations.rs:402-415`；`credential.rs:54-147` 解码） | **OK**（v1 `auth.json` legacy 导入双方都有：ref `migration/20260805200742` ↔ `migrations.rs:470+`） |
| 排序/激活选择 | `asc(active),asc(created),asc(id)`；active 选取 `desc(active),desc(created),desc(id)`；删除后提升 `desc(created),desc(id)`（`credential.ts:84-100,186-192,287-293`） | 同 SQL（`credential.rs:17-22,54-113`） | **OK** |
| create 激活规则 + legacy NULL active 钉住 | `credential.ts:105-168`（`activate!==false || 无现有`；NULL active 钉住） | `credential.rs:153-210` | **OK** |
| 事件 | `credential.updated` 恒发；`credential.switched` 仅激活时（`:160-166`） | `credential.rs:199-208` | **OK** |
| label 派生 | `name / name 2 / name 3…`（`integration.ts:358-367`） | `mod.rs:526-569` | **OK** |
| update（仅 label 变化发 updated） | `credential.ts:205-254` | `credential.rs:258-311` | **OK** |
| remove + 替换提升 + switched(null) | `credential.ts:255-320` | `credential.rs:315-363` | **OK** |
| 多账号切换 | CLI `auth switch` → `credential.activate` | 同（`auth.rs:752-771`） | **OK** |
| export/import | `export.ts` / `import.ts`（secrets 警告、active&&未知集成才激活、跳过已有 id） | `auth.rs:773-890` | **OK**（`auth.rs:874-877` 激活条件一致） |
| 登出 | `logout.ts` → `credential.remove` | `auth.rs:731-750` | **OK**（env 断连提示见 G-3） |
| account 表 | `account/sql.ts`（legacy） | `migrations.rs:140-183` | **OK(EXTRA)** 双方均无消费者 |
| 刷新/过期 | OAuth 5min 窗口刷新、写回、失效文案（`integration.ts:696-714`） | `mod.rs:431-466` + 各 flow refresh | **OK**（copilot `expires:0` 不刷新，双方一致） |

### F. 服务器 HTTP 面

- `oc-server/handlers/integration.rs:54-94`：11 个路由与 v2 对齐（list/get/wellknown/key/oauth connect+status+cancel+complete/command connect+status+cancel）；错误映射（Authorization→400 `integration_authorization`、CodeRequired→400 `integration_code_required`、404 envelope、well-known→400 `well_known_discovery`）逐条对照 v2 handlers。
- `oc-server/handlers/credential.rs:32-38`：list/create/remove/update/activate 齐全。
- `/api/model`、`/api/provider`（`handlers/model.rs:13-19`）：存在；可用性=env-only（B-1）；custom provider 条目补全 `Model.Info` 必填字段并给 openai-compatible 默认 variants（`model.rs:48-76`，对应 `config/plugin/provider.ts:151-159`）。

### G. CLI（oc-cli auth）

| 项 | ref | rust | 结论 |
|---|---|---|---|
| 命令面 | `auth list/login/logout/export/import/switch`（`commands.ts:144-221`） | `AuthCommands`（`cli.rs:302-360`；`auth.rs:147-162`） | **OK**（flags 对齐：`--method`、`--answer`×N、target/credential 参数） |
| 集成解析/方法解析/排序 | `shared.ts:17-66`（opencode-go 置顶、key 方法排最后、id/label 匹配、ambiguous 文案） | `auth.rs:22-137` | **OK**（文案微差：ambiguous 消息格式） |
| readiness 门槛 | `model.default` 先行（`shared.ts:19`） | `auth.rs:24-27` | **OK** |
| login 无 target | 交互式集成选择器（优先级 Popular/MCP/Services，`login.ts:20-104`） | 直接报错要求 target（`auth.rs:253-258`） | **PARTIAL（S2·NEW）** |
| URL 自动打开 | `openUrl(attempt.url)`（`login.ts:177`） | 仅打印（`auth.rs:471-473`） | **PARTIAL（S2·NEW）** |
| 表单交互提示 | `form.ts:12-100`（未知字段/非激活字段报错、select/text/password/external URL、hidden 用 default） | 仅 `--answer`；key 密码与 code 有提示（`auth.rs:401-427,481-484`） | **PARTIAL（S2·NEW）** |
| oauth 等待/取消 | 500ms 轮询、code 模式交互、失败/过期文案、finalizer 取消（`login.ts:150-279`） | 同节奏轮询与取消（`auth.rs:429-531`）；无 Ctrl-C finalizer | **OK（-）** |
| command 登录 stderr 增量打印 | `login.ts:233-249` | `auth.rs:555-608` | **OK** |
| logout env 提示 | action 文案 "log out"（`account.ts:43-49`） | `choose_credential(…, "use", …)`（`auth.rs:727`）→ env 断连提示**永不触发** | **WRONG（S2·NEW）** |
| list 输出 | 表格/json（`list.ts`） | `auth.rs:164-230` | **OK**（列宽逻辑一致） |
| export/import | 见 E 节 | 见 E 节 | **OK** |

---

## 4. 缺失 provider / 缺失登录方式清单

### 4.1 协议不可绑定的 provider（9/226，选中即 `UnsupportedProtocol`）
`perplexity`（`aisdk:@ai-sdk/perplexity`）、`gitlab`（`aisdk:gitlab-ai-provider`）、`salad-cloud`、`watsonx`、`aihubmix`、`merge-gateway`、`v0`（`aisdk:@ai-sdk/vercel`）、`sap-ai-core`（`aisdk:@jerome-benoit/sap-ai-provider-v2`）、`qvac`。
（v2 侧这些走 npm 包动态加载 `provider.ts:112-139`；Rust 无 JS 运行时 —— **KNOWN/设计取舍**，`facade.rs:23-25` 文档化 + 类型化错误 + 测试锁定。）

### 4.2 登录方式（integration methods）对齐表

| integration | v2 方法 | Rust | 备注 |
|---|---|---|---|
| 全部 catalog env-provider | key + env | key + env（azure 过滤对齐） | vertex/bedrock/cloudflare 过滤缺（A-5） |
| opencode | oauth device + key(服务账号) | 同 | OK |
| github-copilot | oauth device（key 移除） | 同 | OK |
| xai | oauth device + key | 同 | 迁移/websocket 缺 |
| openai | chatgpt-browser / chatgpt-headless / chatgpt-token-sharing（+catalog key/env） | 三者俱全 | JWKS KNOWN；hostID 缺 |
| gitlab | oauth pkce | 同 | OK |
| azure | oauth azure-cli（az 存在时）+ key(form) | 同 | 表单条件差 |
| snowflake-cortex | key + env + oauth browser | 同 | OK |
| digitalocean | oauth browser（+catalog key/env） | 同 | OK |
| poe | oauth browser | 有但 **client id/字段错** | D-8 |
| well-known 源 | command `login` | 同 | OK |
| MCP 远程服务器 | oauth（PKCE） | 有（mcp 模块） | 另报 |

**结论：无「整类缺失」的登录方式**；损坏 1 个（Poe），降级 2 个（chatgpt 验签/hostID、xai 迁移）。

---

## 5. 差异索引总表

| ID | 级别 | 状态 | KNOWN/NEW | 摘要 | ref | rust |
|---|---|---|---|---|---|---|
| B-1 | S0 | PARTIAL | NEW | 存储凭据不进入解析/鉴权/可用性（仅 copilot/vertex 特例） | model-resolver.ts:364-371; provider.ts:393-406 | engine/resolver.rs:69-116; copilot.rs:108-119; handlers/model.rs:92-93 |
| B-2 | S0 | PARTIAL | NEW | console 颁发 provider `api_key=None` 匿名请求 | opencode.ts:251,547-552 | console.rs:210; facade.rs:569-576 |
| B-3 | S0 | MISSING | NEW(待运行时复现) | keyless zen 无 `apiKey:"public"`/Auth.none 降级 → 请求鉴权失败 | opencode.ts:317-325; model-resolver.ts:379-385 | facade.rs:476-485; auth.rs:101-112 |
| A-3 | S1 | MISSING | NEW | variant overlay 未并入请求（effort/thinking 不生效） | model-resolver.ts:137-161,196-257 | resolver.rs:154-261; model_request.rs:574-607 |
| A-4 | S1 | WRONG | NEW | config 对 catalog provider：apiKey-only 被丢、baseURL 重绑 openai-compatible | config/plugin/provider.ts; configured.ts:14-18 | overlay.rs:38-117,276-289 |
| A-5 | S1 | WRONG | NEW | env 方法过滤缺 vertex/bedrock/cloudflare；bedrock SigV4/vertex project 激活缺 | plugin/models-dev.ts:77-87; amazon-bedrock.ts:22-45; google-vertex.ts:44-69 | providers.rs:41-68 |
| A-1 | S1 | PARTIAL | NEW | catalog 刷新后 resolver/integration 注册停在 boot 快照 | plugin/models-dev.ts:61-74 | engine/mod.rs:427,455-460,673 |
| D-6 | S1 | MISSING | NEW | ChatGPT/Codex 凭据驱动的 provider 整形（codex baseURL/过滤/成本/头）缺失 | openai.ts:256-299; chatgpt.ts:267-306 | 无（grep 无生产引用） |
| D-8 | S1 | WRONG | NEW | Poe client id 与 token 字段错 → 登录损坏 | poe.ts:11,14-15,154-161 | providers.rs:676,755-756 |
| B-6 | S1 | MISSING | NEW | header/chunk timeout（默认 300s）无执行点 | provider.ts:142-170 | options.rs:130; transport.rs:211-235 |
| P-06 | S1 | MISSING | KNOWN(be3b Ph.3) | copilot 动态模型同步（/models derive、endpoint 选择、gpt-5-chat-latest 隐藏） | github-copilot.ts:172-251; models.ts | 无 |
| P-12 | S1 | MISSING | KNOWN(设计) | npm provider 包运行时安装加载 | provider.ts:112-139 | facade.rs:697-732 类型化错误 |
| A-6 | S2 | MISSING | NEW | `${VAR}` baseURL 替换与 UnresolvedProviderVariablesError | model-resolver.ts:280-306 | 无 |
| A-7 | S2 | WRONG | NEW | model.default 不校验 enabled/available 即返回 | model.ts:260-265 | handlers/model.rs:173-199 |
| A-8 | S2 | MISSING | NEW | 本地 provider 动态发现（ollama/lmstudio/vllm/…） | plugin/provider/local.ts | 无 |
| A-9 | S2 | PARTIAL | NEW | bedrock profile-only 模型与 2 个别名 provider 未过滤 | plugin/models-dev.ts:11-25,94-110 | catalog.rs:137-165 |
| D-4 | S2 | MISSING | KNOWN(be3b Ph.2) | chatgpt id_token RS256/JWKS 验签（仅 claims） | chatgpt.ts:439-459 | providers_enterprise.rs:1252-1280 |
| D-5 | S2 | MISSING | NEW | chatgpt hostID(`ext_agent_host_id`)/savedClientID 复用/旧方法迁移 | chatgpt.ts:100-107,166-172,237-256 | providers_enterprise.rs:1162-1205 |
| D-7 | S2 | MISSING | NEW | xai browser→device 迁移、专属错误文案、JWT exp 回退、websocket 默认 | xai.ts:80-89,98-105,155-160,214-223 | providers.rs:476-584 |
| G-1/2 | S2 | PARTIAL | NEW | CLI 交互集成选择器 / URL 自动打开 / 表单交互提示 | login.ts:62-104,177; form.ts | auth.rs:253-258,471-473 |
| G-3 | S2 | WRONG | NEW | logout 的 env 断连提示 action 文案 "use" → 永不触发 | account.ts:43-49 | auth.rs:727,643 |
| P-17 | S2 | PARTIAL | NEW | OAuth 回调页简化（无品牌/错误详情） | oauth/page.ts | flows.rs:200-229 |
| C-2b | S2 | PARTIAL | NEW | 非默认 models.dev 源的 hash 后缀 KV 键 | models-dev.ts:299-302 | refresh.rs:26 |
| D-9 | S2 | WRONG | NEW | copilot entitlement 文案/`can_signup_for_limited` 分支 | github-copilot.ts:365-370 | providers.rs:358-371 |
| D-10 | S2 | PARTIAL | NEW | azure key 方法表单恒显示（v2 仅未配置时） | azure.ts:125-134 | providers_enterprise.rs:382-387 |

EXTRA（行为超集，非缺陷）：resolver 大小写容差（resolver.rs:124-148）；copilot 显式 http(s) origin 保留（providers.rs:324-334）；`OPENCODE_MODELS_DEV_URL`/`OPENCODE_CATALOG_FETCH` 环境开关（boot.rs:165-181）。

---

## 6. 覆盖清单（审计范围核对）

- [x] provider 解析优先级（CLI/config/env/well-known）：config→overlay（A-4 缺口）；env→catalog env 链（A-5 过滤缺口）；well-known→config Loader 最低层（OK）；CLI `-m`（OK）；**v2 的「存储凭据」优先级整层缺失（B-1）**
- [x] model id 规范化（parse/`#variant`/模式条目/大小写）— A-2 OK / A-10 EXTRA
- [x] 模型能力/限制/成本/variant/reasoning 解析 — 目录侧 OK（C-2）；**请求侧 variant MISSING（A-3）**
- [x] models.dev 加载与缓存刷新（file/cache/snapshot/fetch、TTL、digest、事件、离线）— C-1 OK；刷新传导 A-1
- [x] 自定义 provider / npm 包 — A-4 / P-12
- [x] OAuth：授权 URL、PKCE、回调端口（1455/1457/8080/1456/ephemeral）、轮询（interval+3s、slow_down）、刷新（5min 窗口、旋转、单飞）、过期（attempt 10min/终态 1min/scrub 30s）— D 节
- [x] API key 存储位置与文件格式（DB credential 表 + legacy auth.json 导入）— E 节 OK
- [x] GitHub Copilot token 交换（device flow、entitlement、enterprise、`expires:0`、请求头策略）— OK；动态模型 KNOWN 缺
- [x] credential 多账号（label 派生、active 选择、切换、删除提升）— OK
- [x] 登出/失效处理（remove+替换、needs_auth 状态投影、invalid_grant 文案）— OK（G-3 文案差）
- [x] 错误提示（Authorization/CodeRequired/AttemptNotFound/方法未找到/ambiguous/UnresolvedVariables）— 大体 OK；A-6 缺
- [x] 离线行为（快照兜底、刷新失败静默、console 瞬态失败不改状态）— OK
- [x] 缺失 provider 清单（9 个 aisdk: shim）与缺失登录方式清单（无整类缺失；Poe 损坏）— §4

## 7. 建议优先级（只读建议）

1. **B-1/B-2/B-3**：把 `integration.connection.active+resolve` 通用化进 `CatalogModelResolver`（按 provider.integrationID==providerID 的 catalog 约定），console 投影带 token，zen keyless 注入 `public`/Auth.none —— 一个改动覆盖三个 S0。
2. **A-3**：在 `bind()`/`resolved_from` 中按 `reference.variant` 合并 variant overlay 到 route defaults（复用 `merge_json_records`）。
3. **A-4**：`providers_from_config` 区分「catalog id 命中」→ 生成 `ModelConnection`（apiKey/baseURL 覆盖）而非 CustomProvider。
4. **D-8（Poe）**：两行修复（client id + `api_key`/`api_key_expires_in`）。
5. **A-5/A-1/B-6**：env 过滤表、刷新后重建 integration 注册 + 换 resolver 目录 Arc、transport 层加 header/chunk 超时。
