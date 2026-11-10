# C6b 结论: 凭证/OAuth/集成/模型目录/provider 域移植高度忠实——快照字节级同源、credential/attempt 生命周期与 models-dev 归一化逐行对齐；主要残留为「默认模型无自动回退」「npm-install aisdk: 桥未移植」「wellknown 刷新触发条件比 ref 粗」三点及若干低危顺序/资源差异。

## 对比表

| 域 | ref（file:line） | rust（file:line） | 判定 |
|---|---|---|---|
| Credential CRUD/激活/替换 | credential.ts:84-320 | integration/credential.rs:55-336 | ✅ 对齐（排序/ACTIVE_PICK/G-305 pin/replacement promote/事件发布时机均一致；G-225 decode 失败 fail-read 一致） |
| credential.update 早退 | credential.ts:225（同 label 且无 value → 不写库） | credential.rs:249-271（仍执行 UPDATE 但不 bump 事件） | ≈ 内容等价，见 D-1 |
| 集成注册/方法槽/去重 | integration.ts:296-356 | integration/mod.rs:263-339（same_slot types.rs:221-232） | ✅ oauth/command 按 id、key/env 按型去重一致 |
| 连接投影/list 排序/status 投影 | integration.ts:369-395,680-695 | mod.rs:379-497（credentials 反转+env 附加、name 排序、G-115① metadata） | ✅ |
| connection.resolve + 5min 刷新余量 | integration.ts:696-714 | mod.rs:501-536（REFRESH_MARGIN_MS=5min，refresh 后 update 持久化） | ✅；另增 force-refresh 接缝（G-285②，ref 为 snowflake 401 retry hook） |
| 401 强制刷新 | snowflake-cortex.ts:236-246（经注释） | mod.rs:550-569 connection_force_refresh | ✅ 移植且失败降级为 None（注释声明 ref orDie 会杀流——保守偏差，可接受） |
| connection.key 表单校验/存储 | integration.ts:715-738 | mod.rs:573-607 | ✅（configuration 仅非空 answer） |
| Form 校验（未知键/required/when/format/options） | form.ts:238-370（经注释） | mod.rs:1350-1513（G-260③ neq/format/minLength 等） | ✅ |
| OAuth attempt 生命周期（10min/1min/30s、code-required、already-completing、cancel） | integration.ts:237-239,558-608,762-795 | mod.rs:43-47,683-869 | ✅（G-115③ cancel 经 CancellationToken 中止在途交换） |
| settle/settleCommand 持久化→terminal | integration.ts:408-507 | mod.rs:873-947,1199-1247 | ✅（label 派生 attempt.label ?? impl.label 一致） |
| scrub 过期/清理 | integration.ts:509-556 | mod.rs:1264-1342 | ≈ 见 R-2 |
| command connect（stderr 增量/退出码/空 stdout） | integration.ts:610-674 | mod.rs:958-1132（G-115② 增量 message） | ✅ |
| PKCE S256 / state / pinned 端口 /cancel 探测 | openai.ts:131-138 等（经注释） | flows.rs:34-129 | ✅ |
| 回调页（成功/错误/bootstrap 片段中继） | oauth/page.ts:21-147,276L 全部 | mcp/oauth_page.rs（+flows.rs CallbackPage 四态：Html/Branded/BrandedDeferred/Bootstrap） | ✅ 字面级移植（AUTO_CLOSE 2500ms、scriptString `\u003c`、wordmark/tokens 同源；G-283① success 也 escape） |
| device flow（RFC8628：pending/slow_down/+3s 余量；zen 变体先睡+JSON body+5s 累积退避） | github-copilot.ts:15-16、xai.ts:13、opencode.ts:567-590,644-650（经注释） | flows.rs:552-704（run_device_poll margin=3000；run_device_poll_zen） | ✅ |
| models.dev 内嵌快照同源 | models-dev/snapshot.txt（5,316,382B, md5 fb458b2e…） | provider_catalog/models-dev-snapshot.txt（**md5 相同**） | ✅ 字节级同源 |
| normalize（nativePackage、api→settings、experimental modes、cost/tiers/mergeCost、released、interleaved、openai-compatible→provider id） | models-dev.ts:85-250 | catalog.rs:284-622 + native.rs + source.rs | ✅（Date.parse 手写 civil-days 算法等价；merge_cost tier 键 base:0/context:N 一致） |
| 插件折叠（azure-cognitive-services/google-vertex-anthropic 别名删、bedrock profile-only 13 id 删、openrouter 2 alias 禁用、prompt-cache-key 12 provider、bedrock 链 env 激活、keyless zen 免费层） | plugin/models-dev.ts:11-113、openrouter.ts:16-28、prompt-cache-key.ts:10-23、amazon-bedrock.ts:8-53、opencode.ts:317-342 | catalog.rs:221-282,323-341,468-472,171-212 + providers.rs env 过滤 | ✅（计数锚定 224 providers/8380 models/105 modes） |
| fetch 刷新（TTL 5min、digest 跳过、decode-before-write、Refreshed 事件、失败回退快照） | models-dev.ts:299-434 | refresh.rs:138-266 + engine/mod.rs:803-812（refreshed→refresh_catalog+reload_catalog=CRED-A1） | ✅；ref `options.file` 优先级 → rust 由 embedder adopt 承接（D-5） |
| provider 包加载（builtins 表 53 项 / aisdk: 前缀 / @opencode-ai→@opencode 迁移 / npm add 安装桥） | provider.ts:24-139 | native.rs:21-29,145-198,214-245 | ✅ 表逐项一致；**npm-install 桥未移植**（D-2） |
| provider 可用性（activation/sourceConnection/credential 连接） | provider.ts:384-435 | catalog.rs:141-212 available_models + resolver keyless_downgrade/zen public key | ✅（语义压缩进 catalog+resolver，事件面靠 credential/integration 事件驱动） |
| model-resolver 主链 | model-resolver.ts:137-161,196-345,356-421 | engine/resolver.rs:346-657,796-833 | ✅ withVariant/mergeOverlay/usesAPIKeyAuth(逐字)/hasConfiguredAuth/baseURL ${VAR} 替换在 facade 侧 |
| npm 包名→providerID / 自定义 models 配置覆盖 | config/plugin/provider.ts:56-169 | resolver.rs:383-419（custom）、498-507（CRED-A4 catalog 合并）、overlay.rs:59-151 | ✅ 含 location overlay（LocationModelResolver） |
| 默认模型回退 | model-resolver.ts:402-417（default→available 首个 text 模型） | resolver.rs:660-675（仅 session pin/config default，否则 NotSelected） | ❌ 见 R-1 |
| copilot 目录同步（derive 合并/别名保留/过期删/gpt-5-chat-latest 隐藏/免计费标题 hook） | github-copilot/models.ts:75-234、github-copilot.ts:216-315,402-432 | copilot_sync.rs:44-161,170-268（条目数学在 oc-ai protocols/copilot.rs，A-lane） | ✅ |
| copilot 传输绑定（bearer=GitHub token、api version 2026-08-01、enterprise copilot-api.{domain}、ChatGPT reshape codex/token-sharing） | github-copilot.ts:165-207,359-360、openai.ts:22-27,256-315、chatgpt.ts:23-46,267-306 | integration/copilot.rs:19-74,107-143,208-360 | ✅（codex 白名单 gpt-5.5/spark、denied pro/5.6、gpt-5.<minor>>4 前缀规则、400k/272k 限额、token-sharing 强制 Summary compaction 均在） |
| vertex ADC（authorized_user refresh / service-account RS256 JWT / gCloud well-known 路径） | google-vertex 插件 + google-auth-library | integration/vertex.rs（scope、1h JWT grant） | ✅（协议细节 A-lane 共担） |
| JWKS RS256 ID-token 验证 | chatgpt.ts:439-459（jose） | jwks.rs（ring，每次验证现取 JWKS，30s 超时为自加防御） | ✅ |
| azure 部署发现 / DO routers / modal / local 发现 | azure.ts:179-411、digitalocean.ts:114-180、modal/models.ts 全文、local.ts | azure_sync.rs（ARM+data-plane 双通道）、digitalocean_sync.rs、modal_sync.rs（price ×1e6、limit trunc、effort 去重 null→none、模板 base_model_id 回退）、local.rs | ✅（modal 逐字段对齐，测试锚定 ref 数学） |
| wellknown 配置图（manifest/remote_config {{env:}}/`{env:NAME}` 替换/10min 刷新/变化才通知） | wellknown.ts:31-143、config.ts:130-317 | wellknown_config.rs:16-259 + engine/mod.rs:763-799（600s interval） | ✅ 结构对齐；触发条件差异见 D-3 |
| credential.switched→config 桥（C4 R-7） | config.ts:290-298（**过滤**：switched 的 integrationID ∈ wellknown entries 才 reload） | engine/mod.rs:770-790（Switched/Updated/Integration.updated 一律 refresh；wellknown 无 sources 时内部早退）；server/boot.rs:492-521 integration.updated→config reload | ≈ 功能等价（无 sources 时 no-op），但粒度更粗，见 D-3 |
| account 表 | account/sql.ts（account/account_state/control_account legacy） | rust 侧 database 迁移（C4 lane 已审）；integration 域无引用 | ➖ 未在本 lane 重审（归属 C4） |

## 行为差异清单 D-n

- **D-1** credential.update：ref 在「label 未变且无 value」时跳过 DB 写（credential.ts:225）；rust 仍执行同值 UPDATE（credential.rs:256-270），事件发布条件一致（仅 label 变化发 updated）。结果等价，写放大微小。
- **D-2** `aisdk:` npm-install 桥未移植：ref loadPackage 对非 builtin 包可 `npm.add(root)` 动态安装任意 AI-SDK provider（provider.ts:125-139）；rust `resolve_specifier` 返回 None → 精确报错（native.rs:207-245 注释自认）。非 builtin npm 包在 rust 不可用——snapshot 中此类 provider 落 `aisdk:` 前缀包名，解析将失败（当前此类 provider 均有 API key env → 可用性门先挡住多数路径）。
- **D-3** wellknown 刷新触发条件：ref 仅在 `credential.switched` 且 integrationID 属于已注册 wellknown source 时 reload config（config.ts:290-298）；rust 对 Switched/Updated/Integration.updated 一律跑 refresh_provider_bindings+console+wellknown（engine/mod.rs:770-790）。无 sources 时 no-op 等价；有 sources 时任意无关凭证事件都会触发 manifest 重取+remote_config 重拉（网络放大，行为不变）。C4 R-7 的过滤条件本体未复刻，属「已解决但更粗」。
- **D-4** Resolved Ref 的 variant 归一化：ref `Ref.make` 在未选 variant 时**省略** variant 字段（model-resolver.ts:388-392）；rust `resolved_from` 无条件填 `variant: "default"`（resolver.rs:774-779）。持久化/线面 Ref 形状多一个 `"default"` 字段（TUI/消息记录可见）。
- **D-5** models.dev Options 优先级：ref populate 顺序 file→cache→snapshot→fetch（models-dev.ts:385-401）；rust 为 cache（populate_from_cache）→embedder adopt→fetch loop（refresh.rs:190-266）。`options.url/file/snapshot` 由 embedder 承接，核心语义（cache 先于 snapshot、fetch 刷新叠加）保留。
- **D-6** 动态目录条目顺序：modal derive 用 BTreeMap 按 id 排序输出（modal_sync.rs:327-334），ref Map 保插入序（models.ts:74-83）；copilot 未见过模型同样按 BTreeMap id 序追加。仅影响列表展示顺序，不影响解析。
- **D-7** rust `settle` 失败分支将 `IntegrationError::Authorization` 之外的错误也折叠进 message；ref `message(exit.cause)` 对非 Error 值 `String()` 化——消息文案可能略异，失败语义一致。
- **D-8** oauth_complete 的 "already completing"：ref `Effect.die`（缺陷通道→500 域）；rust 映射 Authorization 400（mod.rs:817-823，注释自认）。HTTP 面行为差异在 oc-server lane 处理。

## 正确性风险 R-n

- **R-1（中）默认模型无「首个可用 text 模型」回退**：ref 在无 config `model` 时回落 `models.default() → available().find(hasPackage && supportsText)`（model-resolver.ts:402-417；model.ts:260-265）；rust `CatalogModelResolver::resolve` 仅接受 session pin 或 config default，否则 `ModelError::NotSelected`（resolver.rs:660-675），`LocationModelResolver` 的 fallback 只覆盖 NotSelected→base（engine 侧 base 同样无 available 回退）。后果：新装环境设了 `ANTHROPIC_API_KEY` 但未配 `model` 时，ref 自动选模型、rust 报未选择。
- **R-2（低）scrub 过期不中止在途 auto 交换**：ref scrub 对过期 pending close scope → fork 的 auto callback 被打断（integration.ts:515-529 + forkIn attemptScope）；rust `scrub_once` 只把条目转 expired，不调用 `pending.cancel`（mod.rs:1264-1303），在途 token 交换继续跑完（settle 因非 pending no-op，不会落凭证）。功能结果一致，资源浪费 + 与 ref 中断语义不同。
- **R-3（低）** oauth_status 对 pending 的 failed 分支：两侧一致——无风险。（保留编号说明已核。）
- **R-4（低）copilot bearer 取 `refresh` 字段**：copilot_binding 以 `oauth.refresh` 作为 bearer（copilot.rs:224，注释引 github-copilot.ts:169,257-261「token 存为 access+refresh」）。若 ref 某版本改为读 access，rust 将发错 token；建议在 A-lane copilot 协议审查中交叉确认。
- **R-5（低）D-2 的运行时放大**：snapshot 中存在 native 无映射的 npm 包 provider 时其模型 package 为 `aisdk:@ai-sdk/...`，用户设 env key 后 provider 变可用、但 bind 时 facade 报 UnsupportedPackage——ref 可走 npm 安装成功。D-2 的用户可见后果，中低用户痛点非数据损坏。

## 覆盖声明

- **已逐行读**：ref credential.ts(325L)、credential/sql.ts、oauth/page.ts(276L)、integration.ts(834L)、models-dev.ts(450L)、provider.ts(473L)、model.ts(306L)、model-resolver.ts(474L)、github-copilot/{models.ts,copilot-provider.ts}、modal/models.ts、account/sql.ts；rust integration/{credential.rs,mod.rs(1513L),flows.rs,types.rs,jwks.rs,copilot.rs,wellknown_config.rs,vertex.rs(头60L)}、provider_catalog/{catalog.rs,native.rs,refresh.rs,support.rs,copilot_sync.rs,modal_sync.rs}、engine/resolver.rs(1-1072/2072，后半为 console/custom 辅助与测试)、mcp/oauth_page.rs(1-120)、engine/mod.rs 与 config.ts/server boot.rs 的凭证桥切片。
- **结构审读（头部+关键 grep，未逐行）**：integration/providers.rs(1017L)、providers_enterprise.rs(1599L)、console.rs(620L)、vertex.rs 剩余、azure_sync.rs、digitalocean_sync.rs、local.rs、overlay.rs、variant.rs、facade.rs(4461L)——对应 ref `plugin/provider/*.ts`（33 文件，非本 lane 指定 ref 范围），其条目数学多处由 oc-core tests 与注释锚定 ref 行号交叉验证。
- **明确不在本 lane**：github-copilot chat/responses 协议与 oc-ai/protocols/copilot.rs（A-lane）；copilot 远端条目字段数学（usable/build/variants 换算）位于 oc-ai protocols/copilot.rs；account 表迁移（C4）；server credential/integration handlers。
- **快照同源**：双侧 snapshot.txt md5 相同（fb458b2e999aeef6f17cad692265d9b4，5,316,382 字节）——条目数/字段抽样由 catalog.rs 测试锚定（224/8380/105 modes、anthropic claude-opus-4-5 限价抽检）。
- **C4 R-7 复核结论**：rust 以「无条件刷新 + wellknown 无源早退 + integration.updated→config reload（server/boot.rs:492-521）」替代 ref 的 per-integration 过滤，功能闭环成立，粒度差异记录为 D-3。
